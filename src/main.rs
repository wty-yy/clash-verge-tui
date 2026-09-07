use anyhow::{bail, Context, Result};
use clap::Parser;
use clash_verge_tui::{
    app::App,
    core::{CoreClient, CoreEvent, Worker},
    core_manager,
    live::{LiveState, ManagedSettings},
    model::{DemoState, Page},
    storage,
    subscriptions::{self},
    ui,
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyEventKind,
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
    /// 使用独立演示模式，不启动 mihomo
    #[arg(long, conflicts_with_all=["core","check","import_only","subscriptions_file","daemon","service"])]
    demo: bool,
    /// 开发兼容选项：导入指定的 v1.19.29 内核到当前工作区
    #[arg(long, hide = true)]
    core: Option<PathBuf>,
    /// 订阅清单 JSON 文件：[{"name":"...","url":"..."}]
    #[arg(long)]
    subscriptions_file: Option<PathBuf>,
    /// 为订阅下载指定代理，例如 http://127.0.0.1:7890
    #[arg(long, requires = "subscriptions_file")]
    subscription_proxy: Option<String>,
    /// 只下载订阅，不启动内核；部分失败时返回非零状态
    #[arg(long, requires = "subscriptions_file", conflicts_with = "check")]
    import_only: bool,
    /// 启动自管内核，读取一次状态并输出 JSON
    #[arg(long)]
    check: bool,
    /// 作为后台服务运行，不打开终端界面
    #[arg(long,conflicts_with_all=["demo","check","snapshot","import_only"])]
    daemon: bool,
    /// 管理当前数据目录的 systemd 用户服务
    #[arg(long,value_parser=["install","start","stop","restart","status","uninstall"],conflicts_with_all=["demo","check","snapshot","import_only","daemon"])]
    service: Option<String>,
    /// 安装、检查或卸载需要系统密码的 TUN 权限服务
    #[arg(long,value_parser=["install","status","uninstall"],conflicts_with_all=["demo","check","snapshot","import_only","daemon","service"])]
    tun_service: Option<String>,
    #[arg(long, hide = true, value_parser=["install","apply","uninstall"])]
    tun_helper: Option<String>,
    #[arg(long, hide = true, requires = "tun_helper")]
    tun_uid: Option<u32>,
    /// 自管内核的代理端口
    #[arg(long)]
    mixed_port: Option<u16>,
    /// 自管内核的控制器端口（仅监听 127.0.0.1）
    #[arg(long)]
    controller_port: Option<u16>,
    /// 启动时使用的已下载订阅编号，从 1 开始
    #[arg(long)]
    profile: Option<usize>,
    /// 自管工作区的数据目录
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// 导出某个页面的确定性快照，不读取或保存用户状态
    #[arg(long,conflicts_with_all=["core","subscriptions_file","check","import_only"],value_parser=["home","proxies","profiles","profile-import","tun-password","connections","rules","logs","unlock","settings"])]
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
            DisableFocusChange,
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
    if !args.check
        && !args.import_only
        && !args.daemon
        && args.service.is_none()
        && args.tun_service.is_none()
        && args.tun_helper.is_none()
        && (!io::stdin().is_terminal() || !io::stdout().is_terminal())
    {
        bail!("交互界面需要终端；静态预览使用 --snapshot home，内核诊断使用 --check");
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
    if let Some(action) = &args.tun_helper {
        clash_verge_tui::service::tun_helper(
            action,
            &dir,
            args.tun_uid.context("TUN 权限助手缺少用户 ID")?,
        )?;
        return Ok(());
    }
    if let Some(action) = &args.tun_service {
        match action.as_str() {
            "install" => {
                let source = runtime.block_on(core_manager::ensure(&dir, args.core.as_deref()))?;
                subscriptions::prepare_binary(&source, &dir.join("core"))?;
                runtime.block_on(clash_verge_tui::service::install_tun_service_cli(&dir))?;
                println!("TUN permission service installed; restart the managed core before use");
            }
            "status" => {
                let ready = clash_verge_tui::service::tun_capable(&dir.join("core/mihomo"));
                println!("{}", if ready { "ready" } else { "not-installed" });
            }
            "uninstall" => {
                runtime.block_on(clash_verge_tui::service::uninstall_tun_service(&dir))?;
                println!("TUN permission service uninstalled");
            }
            _ => unreachable!(),
        }
        return Ok(());
    }
    if let Some(action) = &args.service {
        if action == "install" {
            runtime.block_on(core_manager::ensure(&dir, args.core.as_deref()))?;
            runtime.block_on(clash_verge_tui::service::install(&dir, true))?;
            println!("installed: {}", clash_verge_tui::service::name(&dir));
        } else {
            println!(
                "{}",
                runtime.block_on(clash_verge_tui::service::action(&dir, action))?
            );
        }
        return Ok(());
    }
    if let Some(path) = &args.subscriptions_file {
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
    }
    let mut managed_core = None;
    let mut worker = None;
    let mut app = if args.demo {
        App::new(storage::load(&dir)?, dir)
    } else {
        if args.profile == Some(0) {
            bail!("订阅编号从 1 开始");
        }
        eprintln!("准备 mihomo v{} 工作区…", core_manager::MIHOMO_VERSION);
        let source = runtime.block_on(core_manager::ensure(&dir, args.core.as_deref()))?;
        let running = runtime.block_on(clash_verge_tui::service::open(
            &dir,
            &source,
            args.mixed_port,
            args.controller_port,
            args.profile.map(|number| number - 1),
        ))?;
        let managed = ManagedSettings {
            controller: running.context.controller,
            secret: running.context.secret.clone(),
            port: running.context.port,
            binary: running.context.binary,
        };
        managed_core = running.child;
        let endpoint = running.endpoint;
        let profiles = running.profiles;
        let active = running.active;
        let secret = managed.secret.clone();
        let client = CoreClient::new(&endpoint, secret.clone())?;
        let context = clash_verge_tui::workspace::WorkspaceContext {
            dir: dir.clone(),
            binary: managed.binary.clone(),
            controller: managed.controller.clone(),
            secret: managed.secret.clone(),
            port: managed.port,
        };
        if args.check {
            let snapshot = runtime.block_on(client.snapshot())?;
            let proxies = snapshot.proxies["proxies"].as_object().unwrap();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"app_version":env!("CARGO_PKG_VERSION"),"expected_core":format!("v{}",core_manager::MIHOMO_VERSION),"version":snapshot.version,"mixed_port":snapshot.config["mixed-port"],"proxy_entries":proxies.len(),"groups":proxies.values().filter(|p|p["all"].is_array()).count(),"rules":snapshot.rules["rules"].as_array().unwrap().len(),"connections":snapshot.connections["connections"].as_array().map(Vec::len).unwrap_or(0),"managed":true})
                )?
            );
            return Ok(());
        }
        if !profiles.is_empty() {
            clash_verge_tui::workspace::set_active_on_start(&dir, active)?;
        }
        let snapshot = clash_verge_tui::workspace::load(&dir)?;
        if snapshot
            .state
            .preferences
            .get("system_proxy")
            .is_some_and(|setting| setting == "开启")
        {
            runtime.block_on(clash_verge_tui::platform::apply_proxy(
                &dir,
                &snapshot.state.preferences,
                context.port,
            ))?;
        }
        worker = Some(Worker::spawn_with_workspace(
            CoreClient::new(&endpoint, secret)?,
            Some(context),
        )?);
        App::new_live(dir.clone(), endpoint, profiles, active, Some(managed))?
    };
    if args.daemon {
        runtime.block_on(async {
            let mut term=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            loop {
                tokio::select!{_=tokio::signal::ctrl_c()=>break,_=term.recv()=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{}}
                if let Some(worker)=&worker{for event in worker.events.try_iter().take(64){app.handle_core(event);}for event in worker.logs.try_iter().take(300){app.handle_log(event);}}
                if app.live.as_ref().unwrap().connected {let _=subscriptions::private_write(&app.data_dir.join("daemon.ready"),std::process::id().to_string().as_bytes());}else{let _=std::fs::remove_file(app.data_dir.join("daemon.ready"));}
                if !app.live.as_ref().unwrap().connected {
                    if managed_core.as_mut().is_some_and(|core|!core.is_running()){managed_core.take();}
                    if managed_core.is_none(){if let Ok(source)=core_manager::ensure(&app.data_dir,args.core.as_deref()).await{if let Ok(running)=clash_verge_tui::service::open(&app.data_dir,&source,args.mixed_port,args.controller_port,None).await{let context=running.context;worker=Some(Worker::spawn_with_workspace(CoreClient::new(&running.endpoint,context.secret.clone())?,Some(context.clone()))?);app=App::new_live(context.dir.clone(),running.endpoint,running.profiles,running.active,Some(ManagedSettings{controller:context.controller,secret:context.secret,port:context.port,binary:context.binary}))?;managed_core=running.child;}}}
                }
            }
            Ok::<(),anyhow::Error>(())
        })?;
        drop(worker);
        let _ = std::fs::remove_file(app.data_dir.join("daemon.ready"));
        let _ = runtime.block_on(
            clash_verge_tui::platform::SystemProxy::new(app.data_dir.clone()).restore_if_owned(),
        );
        drop(managed_core);
        return Ok(());
    }
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut last = Instant::now();
    let mut redraw = true;
    let mut last_input = Instant::now();
    let mut focused = true;
    let mut light = false;
    while !app.quit {
        if let Some(live) = app.live.as_mut().filter(|l| !l.connected) {
            if let Some(managed) = live.managed.as_mut() {
                if let Ok(secret) =
                    std::fs::read_to_string(app.data_dir.join("core/controller.secret"))
                {
                    let secret = secret.trim();
                    if !secret.is_empty() && secret != managed.secret {
                        managed.secret = secret.into();
                        let context = clash_verge_tui::workspace::WorkspaceContext {
                            dir: app.data_dir.clone(),
                            binary: managed.binary.clone(),
                            controller: managed.controller.clone(),
                            secret: secret.into(),
                            port: managed.port,
                        };
                        worker = Some(Worker::spawn_with_workspace(
                            CoreClient::new(&live.endpoint, secret.into())?,
                            Some(context),
                        )?);
                        live.pending = false;
                        live.outbox.clear();
                        app.status = "控制器密钥已变更，正在重新连接".into();
                        redraw = true;
                    }
                }
            }
        }
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
        if app.restart_core {
            if let Err(error) =
                restart_after_tun(&runtime, &args, &mut app, &mut worker, &mut managed_core)
            {
                app.restart_core = false;
                app.tun_after_restart = None;
                app.status = format!("TUN 权限已安装，但内核重启失败：{error}");
            }
            redraw = true;
        }
        if let Some(worker) = &worker {
            let delay = app
                .state
                .value("lite_delay")
                .parse::<u64>()
                .unwrap_or(60)
                .max(1);
            let desired = app.state.value("lite") == "开启"
                && (!focused || last_input.elapsed() >= Duration::from_secs(delay));
            if desired != light
                && worker
                    .send(clash_verge_tui::core::Command::PollEvery(if desired {
                        10
                    } else {
                        1
                    }))
                    .is_ok()
            {
                light = desired;
            }
        }
        if redraw && (!light || focused) {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(50))? {
            redraw = true;
            let input = event::read()?;
            if !matches!(input, Event::FocusLost) {
                last_input = Instant::now();
            }
            match input {
                Event::FocusLost => focused = false,
                Event::FocusGained => focused = true,
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
    if managed_core.is_some()
        && runtime.block_on(clash_verge_tui::service::status(&app.data_dir)) != "active"
    {
        let _ = runtime.block_on(
            clash_verge_tui::platform::SystemProxy::new(app.data_dir.clone()).restore_if_owned(),
        );
    }
    drop(managed_core);
    Ok(())
}

fn restart_after_tun(
    runtime: &tokio::runtime::Runtime,
    args: &Args,
    app: &mut App,
    worker: &mut Option<Worker>,
    managed_core: &mut Option<subscriptions::ManagedCore>,
) -> Result<()> {
    let dir = app.data_dir.clone();
    if managed_core.is_some() {
        drop(worker.take());
        drop(managed_core.take());
        let source = runtime.block_on(core_manager::ensure(&dir, args.core.as_deref()))?;
        let running = runtime.block_on(clash_verge_tui::service::open(
            &dir,
            &source,
            args.mixed_port,
            args.controller_port,
            None,
        ))?;
        let context = running.context;
        let endpoint = running.endpoint;
        *worker = Some(Worker::spawn_with_workspace(
            CoreClient::new(&endpoint, context.secret.clone())?,
            Some(context.clone()),
        )?);
        *managed_core = running.child;
        let live = app.live.as_mut().context("TUN 重启缺少真实内核状态")?;
        live.endpoint = endpoint;
        live.connected = false;
        live.pending = false;
        live.outbox.clear();
        live.managed = Some(ManagedSettings {
            controller: context.controller,
            secret: context.secret,
            port: context.port,
            binary: context.binary,
        });
    } else if runtime.block_on(clash_verge_tui::service::status(&dir)) == "active" {
        runtime.block_on(clash_verge_tui::service::action(&dir, "restart"))?;
        runtime.block_on(clash_verge_tui::service::wait_ready(&dir))?;
    } else {
        bail!("找不到需要重启的自管内核或用户服务");
    }
    app.live
        .as_mut()
        .context("TUN 重启缺少真实内核状态")?
        .tun_capable = true;
    app.resume_tun_after_restart();
    Ok(())
}

fn snapshot(args: &Args, page: &str) -> Result<()> {
    let mut app = App::new(DemoState::default(), PathBuf::from("<demo-state>"));
    if page == "profile-import" {
        app.navigate(Page::Profiles);
        app.command('a');
    } else if page == "tun-password" {
        app.live = Some(LiveState::new("unix://<managed>".into(), Vec::new(), None));
        app.tun_password_form();
    } else {
        app.navigate(Page::ALL.into_iter().find(|p| p.slug() == page).unwrap());
    }
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
