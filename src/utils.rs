use clap::Parser;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long)]
    pub homeserver: String,

    #[arg(long, short)]
    pub username: String,

    #[arg(long, short)]
    pub password: String,

    #[arg(long, short)]
    pub config: Vec<String>,

    #[arg(long, short)]
    pub rooms: Vec<String>,
}

