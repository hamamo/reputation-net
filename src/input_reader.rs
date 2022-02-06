use futures::{channel::mpsc::Sender, SinkExt};
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

pub async fn input_reader(mut sender: Sender<String>) -> Result<(), std::io::Error> {
    let mut stdin = BufReader::new(stdin()).lines();
    println!("got stdin()");
    loop {
        match stdin.next_line().await {
            Ok(result) => match result {
                Some(line) => {
                    println!("got line: {}", line);
                    sender.send(line).await.expect("could send");
                }
                None => {
                    println!("EOF on stdin");
                    return Ok(());
                }
            },
            Err(e) => {
                println!("Error {} on stdin", e);
                return Err(e);
            }
        }
    }
}
