//! BlueZ over D-Bus, through the `bluer` crate. Only built on Linux.

use std::collections::HashSet;
use std::time::Duration;

use bluer::agent::Agent;
use bluer::{Adapter, AdapterEvent, Address, Uuid};
use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::SCAN_SECS;
use crate::app::{BtCommand, BtUpdate, FoundSpeaker, Input};

/// The A2DP audio sink service: the device can play music.
const A2DP_SINK: Uuid = Uuid::from_u128(0x0000110b_0000_1000_8000_00805f9b34fb);
/// How often the chosen speaker is checked and reconnected.
const WATCH_SECS: u64 = 10;

pub async fn run(
    watch: Option<String>,
    mut commands: UnboundedReceiver<BtCommand>,
    inputs: UnboundedSender<Input>,
) {
    let send = |update| {
        let _ = inputs.send(Input::Bluetooth(update));
    };
    let session = match bluer::Session::new().await {
        Ok(session) => session,
        Err(e) => {
            tracing::warn!("Bluetooth unavailable: {e}");
            send(BtUpdate::Unavailable);
            return;
        }
    };
    let adapter = match session.default_adapter().await {
        Ok(adapter) => adapter,
        Err(e) => {
            tracing::warn!("no Bluetooth adapter: {e}");
            send(BtUpdate::Unavailable);
            return;
        }
    };
    if let Err(e) = adapter.set_powered(true).await {
        tracing::warn!("powering Bluetooth on failed: {e}");
    }
    // Speakers pair without a PIN; an agent with no callbacks offers exactly that.
    let _agent = match session
        .register_agent(Agent {
            request_default: true,
            ..Default::default()
        })
        .await
    {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!("registering a pairing agent failed: {e}");
            None
        }
    };
    let watch: Option<Address> = watch.and_then(|a| a.parse().ok());
    let mut tick = tokio::time::interval(Duration::from_secs(WATCH_SECS));
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => return,
                Some(BtCommand::Scan) => {
                    scan(&adapter, &send).await;
                    send(BtUpdate::ScanFinished);
                }
                Some(BtCommand::Connect(address)) => match connect(&adapter, &address).await {
                    Ok(()) => send(BtUpdate::Connected(address)),
                    Err(e) => {
                        tracing::warn!("connecting to {address} failed: {e}");
                        send(BtUpdate::ConnectFailed(address));
                    }
                },
                Some(BtCommand::Forget(address)) => {
                    if let Ok(addr) = address.parse::<Address>() {
                        if let Err(e) = adapter.remove_device(addr).await {
                            tracing::warn!("forgetting {address} failed: {e}");
                        }
                    }
                    send(BtUpdate::Forgotten(address));
                }
            },
            _ = tick.tick() => {
                if let Some(addr) = watch {
                    let connected = keep_connected(&adapter, addr).await;
                    let _ = inputs.send(Input::Speaker { connected });
                }
            }
        }
    }
}

async fn scan(adapter: &Adapter, send: &impl Fn(BtUpdate)) {
    if let Err(e) = adapter.set_pairable(true).await {
        tracing::warn!("making Bluetooth pairable failed: {e}");
    }
    let events = match adapter.discover_devices().await {
        Ok(events) => events,
        Err(e) => {
            tracing::warn!("Bluetooth scan failed: {e}");
            return;
        }
    };
    let mut events = std::pin::pin!(events);
    let deadline = tokio::time::sleep(Duration::from_secs(SCAN_SECS));
    let mut deadline = std::pin::pin!(deadline);
    let mut seen = HashSet::new();
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            event = events.next() => match event {
                Some(AdapterEvent::DeviceAdded(addr)) => {
                    if seen.insert(addr) {
                        if let Some(speaker) = describe(adapter, addr).await {
                            send(BtUpdate::Found(speaker));
                        }
                    }
                }
                Some(_) => {}
                None => break,
            },
        }
    }
}

/// Only audio devices are offered: speakers, headphones and the like.
async fn describe(adapter: &Adapter, addr: Address) -> Option<FoundSpeaker> {
    let device = adapter.device(addr).ok()?;
    let icon = device.icon().await.ok().flatten().unwrap_or_default();
    let uuids = device.uuids().await.ok().flatten().unwrap_or_default();
    if !icon.starts_with("audio") && !uuids.contains(&A2DP_SINK) {
        return None;
    }
    let name = device
        .alias()
        .await
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| addr.to_string());
    Some(FoundSpeaker {
        address: addr.to_string(),
        name,
        paired: device.is_paired().await.unwrap_or(false),
    })
}

async fn connect(adapter: &Adapter, address: &str) -> bluer::Result<()> {
    let addr: Address = address.parse().map_err(|_| bluer::Error {
        kind: bluer::ErrorKind::InvalidArguments,
        message: format!("bad address {address}"),
    })?;
    let device = adapter.device(addr)?;
    if !device.is_paired().await? {
        device.pair().await?;
    }
    device.set_trusted(true).await?;
    if !device.is_connected().await? {
        device.connect().await?;
    }
    Ok(())
}

async fn keep_connected(adapter: &Adapter, addr: Address) -> bool {
    let Ok(device) = adapter.device(addr) else {
        return false;
    };
    if device.is_connected().await.unwrap_or(false) {
        return true;
    }
    if let Err(e) = device.connect().await {
        tracing::debug!("speaker {addr} not reachable: {e}");
    }
    device.is_connected().await.unwrap_or(false)
}
