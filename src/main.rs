mod cli;
#[tokio::main]
async fn main() {
    if let Err(error)=cli::execute().await {eprintln!("{error}");std::process::exit(1);}
}
