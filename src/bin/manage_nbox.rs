use clap::Parser;

#[derive(Parser)]
#[command(name = "manage-nbox")]
struct Cli {
    #[command(subcommand)]
    command: nbox::ManageCommand,
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();
    nbox::manage_nbox(cli.command)
}
