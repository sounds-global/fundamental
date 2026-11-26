//! A role on the node that checks passwords is let in by its own password and by nothing else.

use fundamental_db::ConnectionPool;
use fundamental_sql::{SqlConfig, SqlPool};
use fundamental_test_containers::sql::SqlRole;

#[tokio::test]
async fn a_role_connects_with_its_password() {
    let role = SqlRole::mint().await.expect("a role on the auth node");

    let pool = SqlPool::connect(&SqlConfig::development(role.url().expose().as_str()))
        .await
        .expect("the role's own password lets it in");
    pool.health_check().await.expect("and it can query");

    role.release().await.expect("the role is released");
}

#[tokio::test]
async fn a_wrong_password_and_no_password_are_refused() {
    let role = SqlRole::mint().await.expect("a role on the auth node");

    let wrong = role.url_with_password("not-the-password");
    let without = wrong.replacen(":not-the-password@", "@", 1);
    for url in [wrong, without] {
        let refused = match SqlPool::connect(&SqlConfig::development(url.as_str())).await {
            Ok(pool) => pool
                .health_check()
                .await
                .err()
                .map(|error| format!("{error:?}")),
            Err(error) => Some(format!("{error:?}")),
        };
        let refused = refused.expect("a password the role does not have is refused");
        assert!(
            refused.contains("28P01"),
            "refused with SQLSTATE 28P01, invalid_password, not something else: {refused}"
        );
    }

    role.release().await.expect("the role is released");
}
