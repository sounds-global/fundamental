//! A fault run's own engine node restarts, answers again on its new port, and is not the
//! shared node.

use fundamental_test_containers::{DedicatedEngineNode, EngineNode, TcpProxy};
use tokio::net::TcpStream;

#[tokio::test]
async fn a_restarted_node_answers_again_through_a_retargeted_proxy() {
    let mut node = DedicatedEngineNode::start().await.expect("the fault node");
    let proxy = TcpProxy::start(([127, 0, 0, 1], node.sql_port()).into())
        .await
        .expect("a proxy");

    node.restart().await.expect("restarted and answering");
    proxy.retarget(([127, 0, 0, 1], node.sql_port()).into());

    TcpStream::connect(proxy.address())
        .await
        .expect("the proxy reaches the restarted node");
    let shared = EngineNode::shared().await.expect("the shared node");
    assert_ne!(
        shared.sql_port(),
        node.sql_port(),
        "the fault node is its own container"
    );
}
