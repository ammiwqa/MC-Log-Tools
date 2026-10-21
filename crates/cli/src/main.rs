

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use logtools_app::{CommandResult, Presenter};

// =========================
// CLI-clap
// =========================

#[derive(Parser)]
#[command(
    name = "logtools",
    version = "1.0",
    about = "Minecraft LogTools (for server and client logs)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new base from Minecraft log directories
    #[command(aliases = ["cb", "create"])]
    CreateBase {
        /// Name of the new base
        #[arg(required = true, value_name = "NAME")]
        name: String,

        /// Directories with logs (*.log.gz, latest.log)
        #[arg(value_name = "PATH")]
        paths: Vec<String>,

        /// Read paths from text file (one per line, '#' comments)
        #[arg(short, long, value_name = "FILE")]
        from_file: Option<String>,
    },

    /// Search for text in a base (supports & | - ! parens and <globs>)
    #[command(aliases = ["find", "s"])]
    Search {
        /// Base to search
        #[arg(required = true, value_name = "BASE")]
        base_name: String,

        /// Text to search for (supports & | - ! parens and <globs>)
        #[arg(required = true, value_name = "TEXT")]
        text: String,
    },

    /// Update a base if it changed
    #[command(aliases = ["update", "u", "ub"])]
    UpdateBase {
        /// Base to update
        #[arg(required = true, value_name = "BASE")]
        name: String,
    },

    /// List all bases
    #[command(aliases = ["list", "ls", "l"])]
    ListBases,

    /// Find all dirs with Minecraft logs and write them to a file for --from-file.
    /// Using Everything (SDK DLL or es.exe) if available for a whole-PC scan.
    #[command(aliases = ["disc", "d"], verbatim_doc_comment)]
    Discover {
        /// Root directory to scan (default: ~ / user_home). Ignored if Everything used.
        root: Option<String>,

        /// Output file (default: %APPDATA%\\logtools\\results\\mc_log_dirs.txt)
        #[arg(short, long, value_name = "FILE")]
        out: Option<String>,

        /// Path to Everything64.dll or es.exe if not found automatically
        #[arg(long, value_name = "EXE")]
        es: Option<String>,

        /// Skip Everything even if it is available
        #[arg(long)]
        no_everything: bool,
    },

    /// Set / show / list config parameters.
    ///
    /// Set:
    ///   logtools config <param> <var>
    ///
    /// Show one:
    ///   logtools config <param>
    ///
    /// List all:
    ///   logtools config
    ///
    /// Known parameters:
    ///   everything_dll  - path to Everything64.dll for whole-PC discover
    ///   confirm_delete  - ask before deleting a base (true/false, default true)
    #[command(verbatim_doc_comment)]
    Config {
        /// Parameter name
        #[arg(value_name = "PARAM")]
        param: Option<String>,

        /// Value to set
        #[arg(value_name = "VAR")]
        var: Option<String>,
    },

    /// Delete a base (asks for confirmation unless confirm_delete=false in config)
    #[command(aliases = ["del", "rm"])]
    Delete {
        /// Base to delete
        #[arg(required = true, value_name = "NAME")]
        name: String,

        /// Skip confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

// =========================
// CONSOLE PRESENTER
// =========================

struct ConsolePresenter {
    bar: Option<ProgressBar>,
}

impl ConsolePresenter {
    fn new() -> Self {
        Self { bar: None }
    }

    fn make_bar() -> ProgressBar {
        let pb = ProgressBar::new(1);

        pb.set_draw_target(ProgressDrawTarget::stderr_with_hz(50));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("   {prefix} [{bar:30.white/white}] {pos}/{len} [{elapsed_precise}]")
                .unwrap()
                .progress_chars("=>-"),
        );

        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        pb
    }
    fn finish_bar(&mut self) {
        if let Some(pb) = self.bar.take() {
            pb.finish_and_clear();
        }
    }
}

impl Presenter for ConsolePresenter {
    fn progress(&mut self, pos: u64, total: u64) {
        let pb = self
            .bar
            .get_or_insert_with(Self::make_bar);

        pb.set_length(total);
        pb.set_position(pos);
    }

    fn message(&mut self, msg: &str) {
        if let Some(pb) = &mut self.bar {
            pb.set_prefix(msg.to_string());
        }
    }

    fn log(&mut self, line: &str) {
        if let Some(pb) = &self.bar {
            pb.suspend(|| {
                println!("{}", line);
                let _ = std::io::stdout().flush();
            });
        } else {
            println!("{}", line);
        }
    }

    fn error(&mut self, msg: &str) {
        self.finish_bar();
        eprintln!("{}", msg);
    }

    fn confirm(&mut self, prompt: &str) -> bool {
        self.finish_bar();
        print!("{} ", prompt);

        let _ = std::io::stdout().flush();

        let mut answer = String::new();

        if std::io::stdin().read_line(&mut answer).is_err() {
            return false;
        }

        let answer = answer.trim().to_lowercase();

        answer == "y" || answer == "yes"
    }

    fn open_file(&mut self, path: &PathBuf) {
        open_in_system_app(path);
    }
}

fn open_in_system_app(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

fn exit_code_of(result: CommandResult) -> i32 {
    match result {
        CommandResult::Ok => 0,
        CommandResult::ExitCode(code) => code,
        CommandResult::Err(_) => 1,
    }
}

// =========================
// MAIN
// =========================

fn main() {
    let cli = Cli::parse();
    let mut presenter = ConsolePresenter::new();

    let result = match cli.command {
        Commands::CreateBase {
            paths,
            from_file,
            name,
        } => logtools_app::run_create_base(paths, from_file, name, &mut presenter),

        Commands::Search { base_name, text } => {
            logtools_app::run_search(base_name, text, &mut presenter)
        }

        Commands::UpdateBase { name } => logtools_app::run_update_base(name, &mut presenter),

        Commands::ListBases => logtools_app::run_list_bases(&mut presenter),

        Commands::Config { param, var } => {
            logtools_app::run_config(param, var, &mut presenter)
        }

        Commands::Delete { name, yes } => logtools_app::run_delete(name, yes, &mut presenter),

        Commands::Discover {
            root,
            out,
            es,
            no_everything,
        } => logtools_app::run_discover(root, out, es, no_everything, &mut presenter),
    };

    presenter.finish_bar();

    std::process::exit(exit_code_of(result));
}
