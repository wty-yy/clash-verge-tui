//! Run inside an isolated Linux network namespace with loopback and a default route.
use anyhow::Result;
use clash_verge_tui::{
    core::CoreClient,
    subscriptions::{self, ManagedCore, StoredProfile},
    workspace::{self, WorkspaceCommand, WorkspaceContext},
};
use std::{collections::BTreeMap, path::PathBuf};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let binary = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/verge-mihomo"));
    let dir = tempfile::tempdir()?;
    let profile_file = dir.path().join("profiles/profile-0.yaml");
    subscriptions::private_write(&profile_file,b"mode: direct\nproxy-groups:\n  - name: Default\n    type: select\n    proxies: [DIRECT]\nrules: ['MATCH,DIRECT']\n")?;
    let profile = StoredProfile {
        name: "namespace fixture".into(),
        url: String::new(),
        file: profile_file,
        proxies: 0,
        groups: 1,
        user_info: None,
        proxy: None,
    };
    subscriptions::private_write(
        &dir.path().join("profiles/index.json"),
        &serde_json::to_vec(&vec![profile.clone()])?,
    )?;
    let mut core = ManagedCore::start(&binary, &dir.path().join("core"), &profile, 17897, 19097)?;
    let client = CoreClient::new(&core.controller, core.secret.clone())?;
    core.wait_ready(&client).await?;
    let context = WorkspaceContext {
        dir: dir.path().into(),
        binary,
        controller: "127.0.0.1:19097".into(),
        secret: core.secret.clone(),
        port: 17897,
    };
    let settings = BTreeMap::from([
        ("tun".into(), "开启".into()),
        ("tun_device".into(), "cvtun-test".into()),
        ("tun_stack".into(), "system".into()),
        ("auto_route".into(), "开启".into()),
        ("auto_redirect".into(), "关闭".into()),
        ("detect_interface".into(), "关闭".into()),
        ("mtu".into(), "1500".into()),
        ("dns_hijack".into(), String::new()),
    ]);
    workspace::execute(&context, &client, WorkspaceCommand::Settings(settings)).await?;
    assert!(client.get(&["configs"]).await?["tun"]["enable"]
        .as_bool()
        .unwrap_or(false));
    workspace::execute(
        &context,
        &client,
        WorkspaceCommand::Settings(BTreeMap::from([
            ("tun".into(), "关闭".into()),
            ("tun_device".into(), "cvtun-test".into()),
        ])),
    )
    .await?;
    assert!(!client.get(&["configs"]).await?["tun"]["enable"]
        .as_bool()
        .unwrap_or(true));
    workspace::execute(
        &context,
        &client,
        WorkspaceCommand::Settings(BTreeMap::from([
            ("dns".into(), "开启".into()),
            ("dns_listen".into(), "127.0.0.1:15353".into()),
            ("dns_mode".into(), "fake-ip".into()),
            ("fake_range".into(), "198.18.0.1/16".into()),
            ("nameserver".into(), "1.1.1.1".into()),
        ])),
    )
    .await?;
    let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let question =
        b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x07example\x04test\x00\x00\x01\x00\x01";
    socket.send_to(question, "127.0.0.1:15353")?;
    let mut answer = [0u8; 512];
    let (n, _) = socket.recv_from(&mut answer)?;
    assert!(n > question.len());
    assert_eq!(answer[3] & 15, 0);
    workspace::execute(
        &context,
        &client,
        WorkspaceCommand::Settings(BTreeMap::from([
            ("mixed_port".into(), "17901".into()),
            ("allow_lan".into(), "开启".into()),
            ("bind_address".into(), "0.0.0.0".into()),
        ])),
    )
    .await?;
    assert_eq!(
        client.get(&["configs"]).await?["mixed-port"].as_u64(),
        Some(17901)
    );
    let _lan = tokio::net::TcpStream::connect("192.0.2.1:17901").await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18002").await?;
    let echo = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut stream, _) = listener.accept().await?;
        let mut data = [0; 6];
        stream.read_exact(&mut data).await?;
        stream.write_all(&data).await?;
        Ok::<(), std::io::Error>(())
    });
    workspace::execute(&context,&client,WorkspaceCommand::Settings(BTreeMap::from([("tunnels".into(),"- network: [tcp]\n  address: 127.0.0.1:18001\n  target: 127.0.0.1:18002\n  proxy: DIRECT".into())]))).await?;
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect("127.0.0.1:18001").await?;
        stream.write_all(b"tunnel").await?;
        let mut data = [0; 6];
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            stream.read_exact(&mut data),
        )
        .await??;
        assert_eq!(&data, b"tunnel");
    }
    echo.await??;
    workspace::execute(
        &context,
        &client,
        WorkspaceCommand::Settings(BTreeMap::from([
            ("controller".into(), "关闭".into()),
            ("controller_addr".into(), "127.0.0.1:19098".into()),
            ("secret".into(), "new-test-controller-secret".into()),
        ])),
    )
    .await?;
    drop(core);
    let restarted =
        clash_verge_tui::service::open(dir.path(), &context.binary, None, None, None).await?;
    let new_client = CoreClient::new(&restarted.endpoint, restarted.context.secret.clone())?;
    assert!(new_client.get(&["version"]).await.is_ok());
    assert!(std::net::TcpStream::connect("127.0.0.1:19098").is_err());
    println!(
        "TUN, DNS, LAN binding, port changes, TCP tunnels and private-controller restart passed in isolated namespace."
    );
    Ok(())
}
