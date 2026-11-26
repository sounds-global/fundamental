//! A session rotated onto a new credential moves every handle with it, and nothing keeps the
//! credential it replaced.
//!
//! The rotation is driven here rather than by a lease: OpenBao's Cassandra plugin cannot mint a
//! YCQL role.

use std::sync::Arc;
use std::time::Duration;

use fundamental_core::Error as _;
use fundamental_cql::{Cql, CqlConfig, CqlPool, CqlValue, Ddl};
use fundamental_db::Repository;
use fundamental_macros::Model;
use fundamental_test_containers::cql::CqlAuthNode;

/// How long a test waits for the node to stop honouring a dropped role.
const PATIENCE: Duration = Duration::from_secs(60);

/// One reading, written and read through the rotated session.
#[derive(Clone, Debug, PartialEq, Model)]
#[model(cql)]
struct Reading {
    #[model(partition_key)]
    tenant_id: uuid::Uuid,
    #[model(clustering_key)]
    bucket: i64,
    payload: String,
}

/// A login on the node: a role and its password.
struct Login {
    role: String,
    password: String,
}

/// A keyspace of its own, and two logins allowed into it and nowhere else.
struct Fixture {
    node: &'static CqlAuthNode,
    admin: CqlPool,
    keyspace: String,
    first: Login,
    second: Login,
}

impl Fixture {
    async fn up() -> Self {
        let node = CqlAuthNode::shared().await.expect("the auth node");
        let admin = node.admin().await.expect("the superuser connects");
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let keyspace = format!("rot_{}", &suffix[..12]);
        for ddl in [
            format!(
                "CREATE KEYSPACE {keyspace} WITH replication = \
                 {{'class': 'SimpleStrategy', 'replication_factor': 1}}"
            ),
            format!(
                "CREATE TABLE {keyspace}.readings (tenant_id uuid, bucket bigint, payload text, \
                 PRIMARY KEY ((tenant_id), bucket))"
            ),
        ] {
            admin
                .execute_ddl(&Ddl::interpolated(ddl))
                .await
                .expect("the keyspace and its table");
        }
        let mut logins = ["first", "second"].into_iter().map(|which| Login {
            role: format!("{which}_{}", &suffix[..12]),
            password: uuid::Uuid::new_v4().simple().to_string(),
        });
        let (Some(first), Some(second)) = (logins.next(), logins.next()) else {
            unreachable!("two logins were named")
        };
        let fixture = Self {
            node,
            admin,
            keyspace,
            first,
            second,
        };
        for login in [&fixture.first, &fixture.second] {
            fixture.mint(login).await;
        }
        fixture
    }

    async fn mint(&self, login: &Login) {
        for statement in [
            format!(
                "CREATE ROLE {} WITH PASSWORD = '{}' AND LOGIN = true",
                login.role, login.password
            ),
            format!("GRANT ALL ON KEYSPACE {} TO {}", self.keyspace, login.role),
        ] {
            self.admin
                .execute(&Cql::interpolated(statement))
                .await
                .expect("a login");
        }
    }

    /// Drops `login`'s role, as revoking its lease does.
    async fn revoke(&self, login: &Login) {
        self.admin
            .execute(&Cql::interpolated(format!("DROP ROLE {}", login.role)))
            .await
            .expect("the role is dropped");
    }

    fn config(&self, login: &Login) -> CqlConfig {
        self.node
            .config_as(&login.role, &login.password)
            .keyspace(&self.keyspace)
    }

    async fn connect(&self, login: &Login) -> CqlPool {
        CqlPool::connect(&self.config(login))
            .await
            .expect("the login connects")
    }
}

fn reading(tenant: uuid::Uuid, bucket: i64, payload: &str) -> Reading {
    Reading {
        tenant_id: tenant,
        bucket,
        payload: payload.to_owned(),
    }
}

#[tokio::test]
async fn a_repository_taken_before_the_rotation_serves_on_the_new_credential() {
    let fixture = Fixture::up().await;
    let pool = fixture.connect(&fixture.first).await;
    let early = pool.repository::<Reading>();
    let tenant = uuid::Uuid::new_v4();
    early
        .insert(&reading(tenant, 1, "before"))
        .await
        .expect("a write on the first credential");
    assert!(
        pool.cache().size().await > 0,
        "the write prepared a statement"
    );

    let fresh = fixture.connect(&fixture.second).await;
    let replaced = pool.rotate_to(&fresh).await;
    drop(fresh);

    assert_eq!(
        Arc::strong_count(&replaced),
        1,
        "no handle taken before the rotation still holds the session it replaced"
    );
    assert_eq!(
        pool.cache().size().await,
        0,
        "the statements prepared on the replaced session go with it"
    );
    drop(replaced);

    let stranded = fixture
        .connect(&fixture.first)
        .await
        .repository::<Reading>();
    fixture.revoke(&fixture.first).await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        match stranded.insert(&reading(tenant, 9, "stranded")).await {
            Err(_) => break,
            Ok(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Ok(_) => panic!(
                "a session on the dropped role was still served after {}s",
                PATIENCE.as_secs()
            ),
        }
    }

    early
        .insert(&reading(tenant, 2, "after"))
        .await
        .expect("a write through the handle taken before the rotation");
    let read = early
        .query()
        .partition_key(CqlValue::uuid(tenant))
        .execute()
        .await
        .expect("a read first prepared after the rotation");
    let payloads: Vec<&str> = read.iter().map(|r| r.payload.as_str()).collect();
    assert!(
        payloads.starts_with(&["before", "after"]),
        "both writes read back through the early handle: {payloads:?}"
    );
}

#[tokio::test]
async fn the_node_refuses_a_session_without_the_password() {
    let node = CqlAuthNode::shared().await.expect("the auth node");
    let (superuser, _) = node.superuser();

    let refused = CqlPool::connect(&node.config_as(superuser, "not-the-password"))
        .await
        .expect_err("a wrong password is refused");

    assert_eq!(refused.error_code(), "CQL_002", "{refused:?}");
}
