/// The privileges of the connection's own role over one table beyond reading and writing its
/// rows, across every role it is a member of; each lets a process change what a later statement does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per privilege the catalog reports"
)]
pub struct RoleAccess {
    /// The role owns the table, or is a member of the role that does.
    pub owns: bool,
    /// The role is, or is a member of, a superuser.
    pub superuser: bool,
    /// The role is, or is a member of, a role that may create roles, and so grant itself the owner's.
    pub create_role: bool,
    /// The role is, or is a member of, a role that bypasses row-level security.
    pub bypass_row_security: bool,
    /// The role is a member of YugabyteDB's `yb_db_admin`, which may alter objects it does not own.
    pub db_admin: bool,
    /// The role, or a role it is a member of, may create objects in the table's schema.
    pub create_in_schema: bool,
    /// The role, or a role it is a member of, holds `TRIGGER` on the table.
    pub trigger: bool,
    /// The role, or a role it is a member of, holds `TRUNCATE` on the table.
    pub truncate: bool,
    /// The role, or a role it is a member of, holds `REFERENCES` on the table.
    pub references: bool,
    /// The role may execute a `SECURITY DEFINER` function owned by the table's owner or a superuser.
    pub definer_function: bool,
}

impl RoleAccess {
    /// The names of the privileges held, in a fixed order, for a refusal that lists them.
    #[must_use]
    pub fn excess(&self) -> Vec<&'static str> {
        [
            (self.owns, "owns the table"),
            (self.superuser, "superuser"),
            (self.create_role, "CREATEROLE"),
            (self.bypass_row_security, "BYPASSRLS"),
            (self.db_admin, "member of yb_db_admin"),
            (self.create_in_schema, "CREATE on its schema"),
            (self.trigger, "TRIGGER"),
            (self.truncate, "TRUNCATE"),
            (self.references, "REFERENCES"),
            (
                self.definer_function,
                "EXECUTE on a SECURITY DEFINER function of its owner",
            ),
        ]
        .into_iter()
        .filter_map(|(held, name)| held.then_some(name))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::RoleAccess;

    #[test]
    fn a_role_holding_only_dml_has_no_excess() {
        assert!(RoleAccess::default().excess().is_empty());
    }

    #[test]
    fn every_privilege_held_is_named() {
        let all = RoleAccess {
            owns: true,
            superuser: true,
            create_role: true,
            bypass_row_security: true,
            db_admin: true,
            create_in_schema: true,
            trigger: true,
            truncate: true,
            references: true,
            definer_function: true,
        };
        assert_eq!(
            all.excess(),
            [
                "owns the table",
                "superuser",
                "CREATEROLE",
                "BYPASSRLS",
                "member of yb_db_admin",
                "CREATE on its schema",
                "TRIGGER",
                "TRUNCATE",
                "REFERENCES",
                "EXECUTE on a SECURITY DEFINER function of its owner",
            ]
        );
    }
}
