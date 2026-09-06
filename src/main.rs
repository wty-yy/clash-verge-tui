use anyhow::{bail, Context, Result};
use clap::Parser;
use clash_verge_tui::{
    app::App,
    model::{DemoState, Page},
    storage, ui,
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
#[command(version, about = "Clash Verge TUI — 本地演示界面 / UI preview")]
struct Args {
    /// 显式使用演示模式（v0.1.0 唯一模式）
    #[arg(long)]
    demo: bool,
    /// 独立的演示状态目录
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// 导出某个页面的确定性快照，不读取或保存用户状态
    #[arg(long,value_parser=["home","proxies","profiles","connections","rules","logs","unlock","settings"])]
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
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("交互界面需要终端；查看静态预览请使用 --snapshot home");
    }
    let dir = args.data_dir.unwrap_or_else(storage::default_dir);
    let state = storage::load(&dir)?;
    let mut app = App::new(state, dir);
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
    while !app.quit {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if event::poll(Duration::from_millis(50))? {
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
            last = Instant::now();
        }
        if app.dirty {
            storage::save(&app.data_dir, &app.state).context("保存演示状态失败")?;
            app.dirty = false;
        }
    }
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
