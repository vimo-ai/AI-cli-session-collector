//! AI CLI Session 数据结构合规性验证器 CLI

use clap::Parser;
use std::path::PathBuf;
use std::process;

use ai_cli_session_collector::validate::{
    framework::Severity,
    report::{format_human_report, format_json_report, OutputFormat},
    run_claude_validation,
};

#[derive(Parser)]
#[command(
    name = "validate",
    about = "AI CLI Session 数据结构合规性验证器",
    long_about = "验证 AI CLI 会话数据是否符合已知规范。\n\
                  已知结构白名单 + 未知即报警。\n\
                  层级: L0 Field → L1 Line → L2 Block → L3 Turn → L4 Session → L5 Project"
)]
struct Cli {
    /// 数据源类型
    #[arg(long, default_value = "claude")]
    source: String,

    /// 自定义 projects 路径（默认 ~/.claude/projects/）
    #[arg(long)]
    path: Option<PathBuf>,

    /// 单个 session JSONL 文件路径
    #[arg(long)]
    session: Option<PathBuf>,

    /// 最大验证层级 (0-5)
    #[arg(long, default_value = "5")]
    max_level: u8,

    /// 输出格式
    #[arg(long, default_value = "human")]
    format: String,

    /// 详细输出（列出每条 finding）
    #[arg(long, short)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.source != "claude" {
        eprintln!("目前仅支持 --source claude");
        process::exit(1);
    }

    if cli.max_level > 5 {
        eprintln!("--max-level 范围: 0-5");
        process::exit(1);
    }

    let result = run_claude_validation(cli.path, cli.session, cli.max_level);

    let format = match cli.format.as_str() {
        "json" => OutputFormat::Json,
        _ => OutputFormat::Human,
    };

    match format {
        OutputFormat::Human => {
            print!("{}", format_human_report(&result, cli.verbose));
        }
        OutputFormat::Json => {
            println!("{}", format_json_report(&result));
        }
    }

    // 退出码: 有 Error 级别 finding = 1
    if result.findings.iter().any(|f| f.severity == Severity::Error) {
        process::exit(1);
    }
}
