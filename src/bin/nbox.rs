fn main() -> eyre::Result<()> {
    let args = std::env::args_os();
    nbox::nbox(args)
}
