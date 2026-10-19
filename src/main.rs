use std::{borrow::Cow, env, ops::Index, time::Duration};

use color_eyre::eyre::{Context, eyre};
use rand::RngExt;
use tokio::{
    io::{self, AsyncBufReadExt, BufReader},
    select,
    sync::oneshot::{self, Receiver},
    time::sleep,
};

const MESSAGES: &[&str] = &[
    "Error 404: Pizza not found.",
    "Loading the universe... please hold.",
    "Reticulating splines.",
    "Locating the missing socks... Compiling coffee into code.",
    "Everything is fine. Probably.",
    "Did you see that ludicrous display last night? Remember to hydrate today! An apple a day keeps anyone away if you throw it hard enough.",
    "Just general office gossip goes here.",
    "Hello, world! Is anyone out there? Pineapple belongs on pizza.",
    "Fight me.",
    "The narwhal bacons at midnight.",
    "Please do not feed the algorithms.",
    "Look behind you.",
    "Just kidding.",
    "May the forks be with you.",
    "Forty-two.",
    "According to all known laws of aviation, a bee should not be able to fly.",
    "Welcome back, superstar! Your data has been successfully synchronized.",
    "Action item: Click this button.",
    "Synergizing backward overflow.",
    "Optimizing workflow efficiency.",
];

struct WeightedItems(&'static [(u8, usize)]);

impl WeightedItems {
    fn len(&self) -> usize {
        let mut total = 0;

        for item in self.0.iter() {
            total += item.1;
        }

        total
    }
}

impl Index<usize> for WeightedItems {
    type Output = u8;

    fn index(&self, mut index: usize) -> &Self::Output {
        for (item, weight) in self.0 {
            if *weight > index {
                return item;
            }

            index -= weight;
        }

        panic!("index out of bounds");
    }
}

#[allow(dead_code)]
const COLORS: WeightedItems = WeightedItems(&[
    (30, 1),
    (31, 5),
    (32, 5),
    (34, 5),
    (35, 5),
    (36, 5),
    (37, 5),
]);

#[allow(dead_code)]
const FORMATS: WeightedItems = WeightedItems(&[(0, 10), (1, 5), (3, 5), (9, 1)]);

async fn start_logging(mut rx: Receiver<()>) -> color_eyre::Result<()> {
    loop {
        let secs = rand::rng().random_range(1..=8);

        select! {
            _ = sleep(Duration::from_secs(secs)) => {}
            res = &mut rx => {
                return res.wrap_err("Failed to receive oneshot message");
            }
        };

        let mut rng = rand::rng();

        let message = MESSAGES[rng.random_range(0..MESSAGES.len())];

        // let format = FORMATS[rng.random_range(0..FORMATS.len())];

        println!("{message}");
    }
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    println!("Program starting...");

    let (tx, rx) = oneshot::channel::<()>();

    let handle = tokio::spawn(async move { start_logging(rx).await });

    let mut args = env::args();

    let arg1_binding = args.nth(1);

    let echo_prefix = arg1_binding
        .as_deref()
        .unwrap_or("The program echoes back to you ");

    let mut io_reader = BufReader::new(io::stdin());

    loop {
        let mut input = String::new();

        io_reader.read_line(&mut input).await?;

        let input = input.trim_end();

        // println!("*The goofy program decides to process input*");

        if input == "stop" {
            tx.send(())
                .map_err(|_| eyre!("Failed to send to transmitter"))?;
            break;
        }

        let (input, surrounding) = if !input.contains('"') {
            (Cow::Borrowed(input), '"')
        } else if !input.contains('\'') {
            (Cow::Borrowed(input), '\'')
        } else if !input.contains('`') {
            (Cow::Borrowed(input), '`')
        } else {
            (Cow::Owned(input.replace('"', "\\\"")), '"')
        };

        println!("{echo_prefix}{surrounding}{input}{surrounding}");
    }

    handle.await??;

    Ok(())
}
