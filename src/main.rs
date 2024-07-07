use anyhow::Result;
use async_trait::async_trait;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::container::{Config, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use log::{error, info};
use russh::*;
use russh_keys::*;
use std::sync::Arc;
use tokio::net::TcpListener;

use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;

struct Client;

#[async_trait]
impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        info!("check_server_key: {:?}", server_public_key);
        Ok(true)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        info!("data on channel {:?}: {}", channel, data.len());
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    tokio::spawn(async {
        create_socket_forward().await.unwrap();
    });

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/exec", get(exec));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await?;

    Ok(())
}

const IMAGE: &str = "alpine:3";

async fn exec() -> impl IntoResponse {
    let docker =
        Docker::connect_with_http("http://localhost:8181", 4, API_DEFAULT_VERSION).unwrap();

    docker
        .create_image(
            Some(CreateImageOptions {
                from_image: IMAGE,
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await
        .unwrap();

    let alpine_config = Config {
        image: Some(IMAGE),
        tty: Some(true),
        ..Default::default()
    };

    let id = docker
        .create_container::<&str, &str>(None, alpine_config)
        .await
        .unwrap()
        .id;
    docker.start_container::<String>(&id, None).await.unwrap();

    let exec = docker
        .create_exec(
            &id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["ls", "-l", "/"]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
    let mut res = vec![];
    if let StartExecResults::Attached { mut output, .. } =
        docker.start_exec(&exec, None).await.unwrap()
    {
        while let Some(Ok(msg)) = output.next().await {
            info!("{msg}");
            res.push(msg.to_string());
        }
    } else {
        unreachable!();
    }

    docker
        .remove_container(
            &id,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    res.join("\n")
}

async fn create_socket_forward() -> Result<()> {
    let config = russh::client::Config::default();
    let sh = Client {};

    let mut session = russh::client::connect(Arc::new(config), ("192.168.31.214", 22), sh)
        .await
        .unwrap();
    if session
        .authenticate_password("root", "zxcv1234")
        .await
        .unwrap()
    {
        info!("Connected");
        // let mut channel = session.channel_open_session().await.unwrap();
        // channel.exec(true, "pwd").await?;
        //
        // let mut reader = channel.make_reader();
        //
        // let mut contents = Vec::new();
        // reader.read_to_end(&mut contents).await?;
        //
        // info!("content: {}", String::from_utf8_lossy(&contents));

        // listen on a local port
        let listener = TcpListener::bind("127.0.0.1:8181").await?;
        let local_addr = listener.local_addr()?;
        info!("Listening on: {}", local_addr);

        loop {
            let channel = session
                .channel_open_direct_streamlocal("/var/run/docker.sock")
                .await
                .unwrap();

            let (mut read_half, mut write_half) = tokio::io::split(channel.into_stream());

            let (mut socket, addr) = listener.accept().await?;

            tokio::spawn(async move {
                let (mut reader, mut writer) = socket.split();

                tokio::select! {
                    result = tokio::io::copy(&mut reader, &mut write_half) => {
                        match result {
                            Ok(bytes) => info!("Copied {} bytes from reader to write_half", bytes),
                            Err(e) => {
                                error!("Error copying from reader to write_half: {}", e);
                            }
                        }
                    },
                    result = tokio::io::copy(&mut read_half, &mut writer) => {
                        match result {
                            Ok(bytes) => info!("Copied {} bytes from read_half to writer", bytes),
                            Err(e) => {
                                error!("Error copying from read_half to writer: {}", e);
                            }
                        }
                    }
                }
            });
        }
    }

    Ok(())
}
