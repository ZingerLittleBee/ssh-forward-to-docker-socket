use anyhow::Result;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::container::{Config, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
use bollard::{Docker, API_DEFAULT_VERSION};
use log::info;

use crate::ssh::client::ForwardClient;
use futures_util::stream::StreamExt;
use futures_util::TryStreamExt;

mod ssh;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    tokio::spawn(async {
        ForwardClient::new().create_socket_forward().await.unwrap();
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
