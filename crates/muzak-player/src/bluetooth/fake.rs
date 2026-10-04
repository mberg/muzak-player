//! Pretend speakers for `--fake` mode, so the Settings screen can be tried on a Mac.

use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::app::{BtCommand, BtUpdate, FoundSpeaker, Input};

/// Connecting to this one fails, to show the error message.
pub const BROKEN: &str = "00:00:00:00:00:03";

fn speakers() -> Vec<FoundSpeaker> {
    [
        ("00:00:00:00:00:01", "Kitchen Speaker", true),
        ("00:00:00:00:00:02", "Boombox", false),
        (BROKEN, "Old Speaker", false),
    ]
    .into_iter()
    .map(|(address, name, paired)| FoundSpeaker {
        address: address.into(),
        name: name.into(),
        paired,
    })
    .collect()
}

pub async fn run(mut commands: UnboundedReceiver<BtCommand>, inputs: UnboundedSender<Input>) {
    let send = |update| {
        let _ = inputs.send(Input::Bluetooth(update));
    };
    while let Some(command) = commands.recv().await {
        match command {
            BtCommand::Scan => {
                for speaker in speakers() {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    send(BtUpdate::Found(speaker));
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                send(BtUpdate::ScanFinished);
            }
            BtCommand::Connect(address) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if address == BROKEN {
                    send(BtUpdate::ConnectFailed(address));
                } else {
                    send(BtUpdate::Connected(address));
                }
            }
            BtCommand::Forget(address) => send(BtUpdate::Forgotten(address)),
        }
    }
}
