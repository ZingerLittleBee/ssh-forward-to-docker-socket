use std::sync::Arc;
use async_trait::async_trait;
use log::{error, info};
use russh::{ChannelId};
use russh_keys::key;
use tokio::net::TcpListener;

pub struct ForwardClient;

#[async_trait]
impl russh::client::Handler for ForwardClient {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &key::PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        info!("check_server_key: {:?}", server_public_key);
        Ok(true)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> anyhow::Result<(), Self::Error> {
        info!("data on channel {:?}: {}", channel, data.len());
        Ok(())
    }
}

impl ForwardClient {
    pub fn new() -> Self {
        Self
    }
    
    pub async fn create_socket_forward(self) -> anyhow::Result<()> {
        let config = russh::client::Config::default();

        let mut session = russh::client::connect(Arc::new(config), ("192.168.31.214", 22), self)
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

}