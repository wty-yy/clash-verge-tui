use anyhow::{bail, Context, Result};
use clap::Parser;
use clash_verge_tui::{
    app::App,
    core::{CoreClient, CoreEvent, Worker},
    live::ManagedSettings,
    model::{DemoState, Page},
    storage,
    subscriptions::{self, ManagedCore},
    ui,
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    style::Color,
    Terminal,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "Clash Verge TUI — mihomo terminal client")]
struct Args {
    /// 使用独立演示模式（无连接参数时默认）
    #[arg(long, conflicts_with_all=["connect","core","check","import_only","subscriptions_file","secret_file"])]
    demo: bool,
    /// 连接现有 mihomo 外部控制器；关闭界面不会停止该内核
    #[arg(long, conflicts_with = "core")]
    connect: Option<String>,
    /// 启动独立 mihomo；退出界面时停止此子进程
    #[arg(long)]
    core: Option<PathBuf>,
    /// 控制器密钥文件；也可通过 MIHOMO_SECRET 环境变量提供
    #[arg(long, requires = "connect")]
    secret_file: Option<PathBuf>,
    /// 订阅清单 JSON 文件：[{"name":"...","url":"..."}]
    #[arg(long)]
    subscriptions_file: Option<PathBuf>,
    /// 为订阅下载指定代理，例如 http://127.0.0.1:17897
    #[arg(long, requires = "subscriptions_file")]
    subscription_proxy: Option<String>,
    /// 只下载订阅，不启动内核；部分失败时返回非零状态
    #[arg(long, requires="subscriptions_file", conflicts_with_all=["core","connect","check"])]
    import_only: bool,
    /// 读取一次真实内核状态并输出 JSON，适合连接诊断
    #[arg(long)]
    check: bool,
    /// 独立内核的代理端口
    #[arg(long, default_value_t = 17897)]
    mixed_port: u16,
    /// 独立内核的控制器端口（仅监听 127.0.0.1）
    #[arg(long, default_value_t = 19097)]
    controller_port: u16,
    /// 启动时使用的已下载订阅编号，从 1 开始
    #[arg(long, requires = "core")]
    profile: Option<usize>,
    /// 独立的演示状态目录
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// 导出某个页面的确定性快照，不读取或保存用户状态
    #[arg(long,conflicts_with_all=["connect","core","subscriptions_file","check","import_only"],value_parser=["home","proxies","profiles","connections","rules","logs","unlock","settings"])]
    snapshot: Option<String>,
    /// 快照输出路径；.svg 输出彩色 SVG，其他后缀输出文本
    #[arg(long, requires = "snapshot")]
    output: Option<PathBuf>,
    #[arg(long,default_value_t=120,value_parser=clap::value_parser!(u16).range(20..=300))]
    width: u16,
    #[arg(long,default_value_t=40,value_parser=clap::value_parser!(u16).range(10..=120))]
    height: u16,
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}
fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(ref page) = args.snapshot {
        return snapshot(&args, page);
    }
    if args.check && args.connect.is_none() && args.core.is_none() {
        bail!("--check 需要 --connect 或 --core");
    }
    if !args.check
        && !args.import_only
        && (!io::stdin().is_terminal() || !io::stdout().is_terminal())
    {
        bail!("交互界面需要终端；静态预览使用 --snapshot home，连接诊断使用 --connect URL --check");
    }
    let dir = args.data_dir.clone().unwrap_or_else(storage::default_dir);
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(dir)
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if let Some(path) = &args.subscriptions_file {
        if !args.import_only && args.core.is_none() {
            bail!("订阅导入请使用 --import-only，或同时使用 --core 启动独立内核");
        }
        let results = runtime.block_on(subscriptions::import_file(
            path,
            &dir.join("profiles"),
            args.subscription_proxy.as_deref(),
        ))?;
        let mut failed = 0;
        for (i, result) in results.iter().enumerate() {
            match &result.result {
                Ok(profile) => eprintln!(
                    "订阅 {}：已导入 {} 个节点、{} 个策略组",
                    i + 1,
                    profile.proxies,
                    profile.groups
                ),
                Err(error) => {
                    failed += 1;
                    eprintln!("订阅 {}：{}", i + 1, error);
                }
            }
        }
        if args.import_only {
            if failed > 0 {
                bail!("{failed} 个订阅导入失败；成功下载的订阅已保留");
            }
            return Ok(());
        }
        if args.core.is_none() && args.connect.is_none() {
            bail!("已导入订阅；使用 --core 启动，或添加 --import-only 仅导入");
        }
    }
    let mut managed_core = None;
    let mut worker = None;
    let mut app = if args.core.is_some() || args.connect.is_some() {
        let profiles = if args.core.is_some() {
            subscriptions::load_profiles(&dir.join("profiles"))?
        } else {
            Vec::new()
        };
        let stored_active = std::fs::read_to_string(dir.join("profiles/active.json"))
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let active = match args.profile {
            Some(0) => bail!("订阅编号从 1 开始"),
            Some(n) => n - 1,
            None => stored_active,
        };
        let (endpoint, secret, managed) = if let Some(binary) = &args.core {
            let profile = profiles.get(active).context(
                "没有对应的订阅；先通过 --subscriptions-file 导入，或检查 --profile 编号",
            )?;
            let core = ManagedCore::start(
                binary,
                &dir.join("core"),
                profile,
                args.mixed_port,
                args.controller_port,
            )?;
            let endpoint = core.controller.clone();
            let secret = core.secret.clone();
            let managed = ManagedSettings {
                controller: format!("127.0.0.1:{}", args.controller_port),
                secret: secret.clone(),
                port: args.mixed_port,
            };
            managed_core = Some(core);
            (endpoint, secret, Some(managed))
        } else {
            let secret = if let Some(path) = &args.secret_file {
                std::fs::read_to_string(path)
                    .context("无法读取控制器密钥文件")?
                    .trim()
                    .to_string()
            } else {
                std::env::var("MIHOMO_SECRET").unwrap_or_default()
            };
            (args.connect.clone().unwrap(), secret, None)
        };
        let client = CoreClient::new(&endpoint, secret.clone())?;
        if let Some(core) = managed_core.as_mut() {
            eprintln!("正在启动独立 mihomo…");
            runtime.block_on(core.wait_ready(&client))?;
        }
        if args.check {
            let snapshot = runtime.block_on(client.snapshot())?;
            let proxies = snapshot.proxies["proxies"].as_object().unwrap();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"version":snapshot.version,"proxy_entries":proxies.len(),"groups":proxies.values().filter(|p|p["all"].is_array()).count(),"rules":snapshot.rules["rules"].as_array().unwrap().len(),"connections":snapshot.connections["connections"].as_array().map(Vec::len).unwrap_or(0),"managed":managed_core.is_some()})
                )?
            );
            return Ok(());
        }
        worker = Some(Worker::spawn(CoreClient::new(&endpoint, secret)?)?);
        App::new_live(dir.clone(), endpoint, profiles, active, managed)?
    } else {
        App::new(storage::load(&dir)?, dir)
    };
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut last = Instant::now();
    let mut redraw = true;
    while !app.quit {
        if let Some(worker) = &worker {
            for event in worker.events.try_iter().take(64) {
                app.handle_core(event);
                redraw = true;
            }
            for event in worker.logs.try_iter().take(300) {
                app.handle_log(event);
                redraw = true;
            }
            let commands = std::mem::take(&mut app.live.as_mut().unwrap().outbox);
            for command in commands {
                if let Err(error) = worker.send(command) {
                    app.handle_core(CoreEvent::Completed(Err(error.to_string())));
                }
            }
        }
        if redraw {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(50))? {
            redraw = true;
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                Event::Mouse(mouse) => app.mouse(mouse),
                Event::Paste(text) => app.paste(&text),
                Event::Resize(_, _) => app.cancel_pending_click(),
                _ => {}
            }
        }
        let interval = app
            .state
            .value("refresh")
            .parse::<u64>()
            .unwrap_or(1000)
            .max(100);
        if last.elapsed() >= Duration::from_millis(interval) {
            app.tick();
            if app.live.is_none() {
                redraw = true;
            }
            last = Instant::now();
        }
        if app.dirty {
            if app.live.is_some() {
                app.save_preferences().context("保存界面偏好失败")?;
            } else {
                storage::save(&app.data_dir, &app.state).context("保存演示状态失败")?;
            }
            app.dirty = false;
        }
    }
    drop(worker);
    drop(managed_core);
    Ok(())
}
fn snapshot(args: &Args, page: &str) -> Result<()> {
    let mut app = App::new(DemoState::default(), PathBuf::from("<demo-state>"));
    app.navigate(Page::ALL.into_iter().find(|p| p.slug() == page).unwrap());
    let mut terminal = Terminal::new(TestBackend::new(args.width, args.height))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let buffer = terminal.backend().buffer();
    let out = if args
        .output
        .as_ref()
        .and_then(|p| p.extension())
        .is_some_and(|x| x == "svg")
    {
        svg(buffer)
    } else {
        plain(buffer)
    };
    if let Some(path) = &args.output {
        std::fs::write(path, out).with_context(|| format!("写入 {} 失败", path.display()))?;
    } else {
        println!("{out}");
    }
    Ok(())
}
fn plain(buffer: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buffer.area.height {
        let mut skip = 0;
        let mut line = String::new();
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buffer[(x, y)].symbol();
            line.push_str(symbol);
            skip = unicode_width::UnicodeWidthStr::width(symbol).saturating_sub(1);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn color(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => "#e0e4f1".into(),
        _ => "#8993ac".into(),
    }
}
fn svg(buffer: &Buffer) -> String {
    let w = buffer.area.width as usize * 10 + 32;
    let h = buffer.area.height as usize * 20 + 32;
    let mut s=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><title>Clash Verge TUI — actual terminal buffer</title><rect width=\"100%\" height=\"100%\" fill=\"#10131c\"/><g font-family=\"DejaVu Sans Mono, Noto Sans Mono CJK SC, monospace\" font-size=\"15\">");
    for y in 0..buffer.area.height {
        let mut skip = 0;
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let c = &buffer[(x, y)];
            let width = unicode_width::UnicodeWidthStr::width(c.symbol()).max(1);
            skip = width - 1;
            let bg = if c.bg == Color::Reset {
                "#10131c".into()
            } else {
                color(c.bg)
            };
            s.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"20\" fill=\"{}\"/>",
                16 + x as usize * 10,
                16 + y as usize * 20,
                width * 10,
                bg
            ));
        }
    }
    for y in 0..buffer.area.height {
        let mut skip = 0;
        for x in 0..buffer.area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let c = &buffer[(x, y)];
            let width = unicode_width::UnicodeWidthStr::width(c.symbol());
            skip = width.saturating_sub(1);
            if c.symbol().trim().is_empty() {
                continue;
            }
            s.push_str(&format!("<text x=\"{}\" y=\"{}\" fill=\"{}\" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",16+x as usize*10,31+y as usize*20,color(c.fg),width*10,xml(c.symbol())));
        }
    }
    s.push_str("</g></svg>\n");
    s
}
