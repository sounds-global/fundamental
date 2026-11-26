use fundamental_core::model::sql::ShardKind;

use super::{Snapshot, UndeclaredChange};
use crate::migration::Lints;

fn next_statements(
    before: &Snapshot,
    models: &Snapshot,
) -> Result<Vec<String>, Vec<UndeclaredChange>> {
    super::next_statements(before, models, &Lints::default())
}

use crate::migration::{KeyPart, TableColumn, TableIndex, TableSchema};

fn column(name: &str, data_type: &str, nullable: bool, default: Option<&str>) -> TableColumn {
    TableColumn {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        default: default.map(str::to_owned),
        primary_key: false,
        unique: false,
    }
}

fn articles(more: impl FnOnce(&mut TableSchema)) -> TableSchema {
    let mut table = TableSchema::new("articles");
    table.add_column(column("organisation_id", "UUID", false, None));
    table.add_column(column("id", "UUID", false, None));
    table.add_column(column("headline", "TEXT", false, None));
    table.set_key(KeyPart::parts(&[
        ("organisation_id", Some(ShardKind::Hash)),
        ("id", Some(ShardKind::Asc)),
    ]));
    more(&mut table);
    table
}

fn by_headline() -> TableIndex {
    TableIndex {
        name: "articles_by_headline".to_owned(),
        columns: Vec::new(),
        unique: false,
        condition: None,
        key: KeyPart::parts(&[
            ("organisation_id", Some(ShardKind::Hash)),
            ("headline", Some(ShardKind::Asc)),
        ]),
        include: Vec::new(),
        constraint: false,
    }
}

fn headline_once() -> TableIndex {
    TableIndex {
        name: "articles_headline_once".to_owned(),
        unique: true,
        constraint: true,
        ..by_headline()
    }
}

fn snapshot(tables: &[TableSchema]) -> Snapshot {
    Snapshot::of(tables, None)
}

#[test]
fn a_new_table_is_created_with_its_indexes() {
    let statements = next_statements(
        &Snapshot::default(),
        &snapshot(&[articles(|table| table.add_index(by_headline()))]),
    )
    .expect("written");
    assert_eq!(statements.len(), 2);
    assert!(statements[0].starts_with("CREATE TABLE IF NOT EXISTS articles ("));
    assert!(statements[1].starts_with("CREATE INDEX IF NOT EXISTS articles_by_headline"));
}

#[test]
fn models_that_match_their_migrations_need_none() {
    let both = snapshot(&[articles(|_| {})]);
    assert_eq!(next_statements(&both, &both), Ok(Vec::new()));
}

#[test]
fn a_new_column_and_index_are_added_to_a_table_that_has_rows() {
    let statements = next_statements(
        &snapshot(&[articles(|_| {})]),
        &snapshot(&[articles(|table| {
            table.add_column(column("standfirst", "TEXT", false, Some("''")));
            table.add_column(column("published_on", "DATE", true, None));
            table.add_index(by_headline());
        })]),
    )
    .expect("written");
    assert_eq!(
        statements,
        [
            "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT NOT NULL DEFAULT '';",
            "ALTER TABLE articles ADD COLUMN IF NOT EXISTS published_on DATE;",
            "CREATE INDEX IF NOT EXISTS articles_by_headline\n    \
             ON articles (organisation_id HASH, headline ASC);",
        ]
    );
}

#[test]
fn a_new_check_is_added_and_a_changed_one_refused() {
    let checked = |expression: &str| {
        let expression = expression.to_owned();
        articles(move |table| {
            table.add_check_constraint(crate::migration::TableCheckConstraint {
                name: "articles_headline".to_owned(),
                expression,
            });
        })
    };
    assert_eq!(
        next_statements(
            &snapshot(&[articles(|_| {})]),
            &snapshot(&[checked("headline <> ''")])
        ),
        Ok(vec![
            "ALTER TABLE articles ADD CONSTRAINT articles_headline CHECK (headline <> '');"
                .to_owned()
        ])
    );
    let undeclared = next_statements(
        &snapshot(&[checked("headline <> ''")]),
        &snapshot(&[checked("char_length(headline) > 2")]),
    )
    .expect_err("refused");
    assert!(
        undeclared[0].reason.contains("`articles_headline` changed"),
        "{undeclared:?}"
    );
}

#[test]
fn what_would_lose_data_or_rewrite_a_table_is_refused_with_what_to_declare() {
    let before = snapshot(&[articles(|table| {
        table.add_column(column("body", "TEXT", false, None));
        table.add_index(by_headline());
    })]);
    let undeclared = next_statements(
        &before,
        &snapshot(&[articles(|table| {
            table.add_column(column("kind", "TEXT", false, None));
        })]),
    )
    .expect_err("refused");
    let reasons: Vec<&str> = undeclared
        .iter()
        .map(|change| change.reason.as_str())
        .collect();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("`kind` is added NOT NULL with no default")),
        "{reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("`body` is gone")),
        "{reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("`articles_by_headline` is gone")),
        "{reasons:?}"
    );

    let retyped = next_statements(
        &snapshot(&[articles(|_| {})]),
        &snapshot(&[articles(|table| {
            table
                .columns
                .get_mut("headline")
                .expect("a column")
                .data_type = "INT8".to_owned();
        })]),
    )
    .expect_err("refused");
    assert!(
        retyped[0].reason.contains("`headline` changed"),
        "{retyped:?}"
    );

    let dropped =
        next_statements(&snapshot(&[articles(|_| {})]), &Snapshot::default()).expect_err("refused");
    assert!(dropped[0].reason.contains("table is gone"), "{dropped:?}");
}

#[test]
fn a_column_the_model_says_was_named_otherwise_is_renamed() {
    let before = snapshot(&[articles(|table| {
        table.add_index(by_headline());
        table.add_check_constraint(crate::migration::TableCheckConstraint {
            name: "articles_headline".to_owned(),
            expression: "headline <> ''".to_owned(),
        });
    })]);
    let renamed = snapshot(&[articles(|table| {
        table.columns.remove("headline");
        table.column_order.retain(|name| name != "headline");
        table.add_column(column("title", "TEXT", false, None));
        table.rename_column("title", "headline");
        let mut index = by_headline();
        index.key[1].columns = vec!["title".to_owned()];
        table.add_index(index);
        table.add_check_constraint(crate::migration::TableCheckConstraint {
            name: "articles_headline".to_owned(),
            expression: "title <> ''".to_owned(),
        });
    })]);
    assert_eq!(
        next_statements(&before, &renamed),
        Ok(vec![
            "ALTER TABLE articles RENAME COLUMN headline TO title;".to_owned()
        ]),
        "its index and check follow it, as the engine renames them"
    );
}

#[test]
fn a_renamed_column_that_also_changed_is_refused() {
    let undeclared = next_statements(
        &snapshot(&[articles(|_| {})]),
        &snapshot(&[articles(|table| {
            table.columns.remove("headline");
            table.column_order.retain(|name| name != "headline");
            table.add_column(column("title", "TEXT", true, None));
            table.rename_column("title", "headline");
        })]),
    )
    .expect_err("refused");
    assert!(
        undeclared[0]
            .reason
            .contains("`title`, which was `headline`, changed"),
        "{undeclared:?}"
    );
}

#[test]
fn two_columns_renamed_from_one_name_are_refused() {
    let undeclared = next_statements(
        &snapshot(&[articles(|_| {})]),
        &snapshot(&[articles(|table| {
            table.columns.remove("headline");
            table.column_order.retain(|name| name != "headline");
            table.add_column(column("title", "TEXT", false, None));
            table.add_column(column("heading", "TEXT", true, None));
            table.rename_column("title", "headline");
            table.rename_column("heading", "headline");
        })]),
    )
    .expect_err("refused");
    assert!(
        undeclared.iter().any(|change| change
            .reason
            .contains("`heading` and `title` both say they `was` `headline`")),
        "{undeclared:?}"
    );
}

#[test]
fn a_column_index_or_check_both_declared_and_retired_is_refused() {
    let declared = |table: &mut TableSchema| {
        table.add_index(by_headline());
        table.add_check_constraint(crate::migration::TableCheckConstraint {
            name: "articles_headline".to_owned(),
            expression: "headline <> ''".to_owned(),
        });
    };
    let before = snapshot(&[articles(declared)]);
    let both = snapshot(&[articles(|table| {
        declared(table);
        table.retire_column("headline");
        table.retire_index("articles_by_headline");
        table.retire_check("articles_headline");
    })]);
    let reasons: Vec<String> = next_statements(&before, &both)
        .expect_err("refused")
        .into_iter()
        .map(|change| change.reason)
        .collect();
    for both_ways in [
        "column `headline` is both declared and retired",
        "index `articles_by_headline` is both declared and retired",
        "check `articles_headline` is both declared and retired",
    ] {
        assert!(
            reasons.iter().any(|reason| reason.contains(both_ways)),
            "{both_ways}: {reasons:?}"
        );
    }
}

#[test]
fn what_the_model_retires_is_dropped_after_what_it_adds() {
    let before = snapshot(&[articles(|table| {
        table.add_column(column("body", "TEXT", false, None));
        table.add_index(by_headline());
        table.add_check_constraint(crate::migration::TableCheckConstraint {
            name: "articles_body".to_owned(),
            expression: "body <> ''".to_owned(),
        });
    })]);
    let retiring = snapshot(&[articles(|table| {
        table.add_column(column("standfirst", "TEXT", true, None));
        table.retire_column("body");
        table.retire_index("articles_by_headline");
        table.retire_check("articles_body");
    })]);
    assert_eq!(
        next_statements(&before, &retiring),
        Ok(vec![
            "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT;".to_owned(),
            "ALTER TABLE articles DROP CONSTRAINT IF EXISTS articles_body;".to_owned(),
            "DROP INDEX IF EXISTS articles_by_headline;".to_owned(),
            "ALTER TABLE articles DROP COLUMN IF EXISTS body;".to_owned(),
        ])
    );
    assert_eq!(
        next_statements(&snapshot(&[articles(|_| {})]), &retiring),
        Ok(vec![
            "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT;".to_owned()
        ]),
        "what is already gone needs nothing"
    );
}

#[test]
fn a_unique_index_a_constraint_made_is_dropped_as_the_constraint() {
    let before = snapshot(&[articles(|table| table.add_index(headline_once()))]);
    let file = before.to_file();
    assert!(file.contains("\"constraint\": true"), "{file}");
    assert_eq!(
        Snapshot::from_file(&file).expect("a snapshot"),
        before,
        "it reads back a constraint's"
    );
    assert_eq!(
        next_statements(
            &before,
            &snapshot(&[articles(
                |table| table.retire_index("articles_headline_once")
            )])
        ),
        Ok(vec![
            "ALTER TABLE articles DROP CONSTRAINT IF EXISTS articles_headline_once;".to_owned()
        ]),
        "the engine refuses to drop an index a constraint needs"
    );
}

#[test]
fn a_new_unique_index_is_never_a_constraint() {
    let declared = snapshot(&[articles(|table| table.add_index(headline_once()))]);
    for before in [Snapshot::default(), snapshot(&[articles(|_| {})])] {
        let refused = next_statements(&before, &declared).expect_err("refused");
        assert!(
            refused.iter().any(|change| change
                .reason
                .contains("`articles_headline_once` is a constraint's")),
            "{refused:?}"
        );
    }
}

#[test]
fn a_table_the_owner_retires_is_dropped_last() {
    let mut drafts = TableSchema::new("drafts");
    drafts.add_column(column("id", "UUID", false, None));
    drafts.set_key(KeyPart::parts(&[("id", Some(ShardKind::Hash))]));
    let before = snapshot(&[articles(|_| {}), drafts]);
    let models = snapshot(&[articles(|table| {
        table.add_column(column("standfirst", "TEXT", true, None));
    })])
    .retiring(&["drafts"]);
    assert_eq!(
        next_statements(&before, &models),
        Ok(vec![
            "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT;".to_owned(),
            "DROP TABLE IF EXISTS drafts;".to_owned(),
        ])
    );
    let both = snapshot(&[articles(|_| {})]).retiring(&["articles"]);
    let refused = next_statements(&before, &both).expect_err("refused");
    assert!(
        refused
            .iter()
            .any(|change| change.reason.contains("both declared and retired")),
        "{refused:?}"
    );
}

#[test]
fn what_is_retired_or_renamed_is_never_written_to_the_snapshot_file() {
    let models = snapshot(&[articles(|table| {
        table.retire_column("body");
        table.rename_column("headline", "title");
    })])
    .retiring(&["drafts"]);
    let file = models.to_file();
    assert!(
        !file.contains("body") && !file.contains("title") && !file.contains("drafts"),
        "{file}"
    );
}

#[test]
fn a_snapshot_reads_back_from_its_file_as_it_was_written() {
    let written = Snapshot::of(
        &[articles(|table| table.add_index(by_headline()))],
        Some("sha256:abc".to_owned()),
    );
    let file = written.to_file();
    assert_eq!(Snapshot::from_file(&file).expect("a snapshot"), written);
    assert_eq!(
        Snapshot::from_file(&file).expect("a snapshot").to_file(),
        file,
        "the file is canonical"
    );
}

mod owner {
    use super::{articles, column};
    use crate::migration::generate::owner::{Generation, rebase_on};
    use crate::migration::generate::{
        GenerateError, Main, Snapshot, check, rebase, regenerate, write_baseline, write_next,
    };
    use crate::migration::{EmbeddedMigration, EmbeddedMigrations, TableSchema, content_hash};

    fn leak(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    fn plain() -> TableSchema {
        articles(|_| {})
    }

    fn with_standfirst() -> TableSchema {
        articles(|table| table.add_column(column("standfirst", "TEXT", false, Some("''"))))
    }

    fn set(
        dir: &std::path::Path,
        files: &[(&str, String)],
        tables: &'static [fn() -> TableSchema],
    ) -> EmbeddedMigrations {
        let migrations: Vec<EmbeddedMigration> = files
            .iter()
            .map(|(name, up)| EmbeddedMigration::new(leak((*name).to_owned()), leak(up.clone())))
            .collect();
        let snapshot =
            std::fs::read_to_string(dir.join("migrations/schema.json")).unwrap_or_default();
        EmbeddedMigrations::sql("articles", Box::leak(migrations.into_boxed_slice()), tables)
            .generated(leak(snapshot), leak(dir.display().to_string()))
    }

    #[test]
    fn a_model_changed_without_its_migration_is_found_and_written_as_the_next_file() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("migrations")).expect("migrations");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());

        write_baseline(&set(dir.path(), std::slice::from_ref(&first), &[plain]))
            .expect("a baseline");
        check(&set(dir.path(), std::slice::from_ref(&first), &[plain]))
            .expect("the models are their migrations");

        let changed = set(dir.path(), std::slice::from_ref(&first), &[with_standfirst]);
        match check(&changed) {
            Err(GenerateError::Pending { statements, .. }) => assert_eq!(
                statements,
                [
                    "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT NOT NULL DEFAULT '';"
                ]
            ),
            other => panic!("pending, not {other:?}"),
        }
        let written = write_next(&changed, "article_standfirst").expect("written");
        assert!(
            written.ends_with("migrations/sql/0002_article_standfirst.up.sql"),
            "{}",
            written.display()
        );
        let second = std::fs::read_to_string(&written).expect("the file");
        assert!(
            second.contains("ADD COLUMN IF NOT EXISTS standfirst"),
            "{second}"
        );

        let after = set(
            dir.path(),
            &[first.clone(), ("0002_article_standfirst", second)],
            &[with_standfirst],
        );
        check(&after).expect("the new file and snapshot agree with the models");
        assert!(matches!(
            write_next(&after, "again"),
            Err(GenerateError::NothingToWrite { .. })
        ));
    }

    #[test]
    fn a_file_added_beside_the_generator_makes_the_snapshot_stale() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("migrations")).expect("migrations");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        write_baseline(&set(dir.path(), std::slice::from_ref(&first), &[plain]))
            .expect("a baseline");
        let by_hand = (
            "0002_by_hand",
            "ALTER TABLE articles ADD COLUMN x TEXT;".to_owned(),
        );
        assert!(matches!(
            check(&set(dir.path(), &[first, by_hand], &[plain])),
            Err(GenerateError::Stale { .. })
        ));
    }

    fn drafts() -> TableSchema {
        let mut table = TableSchema::new("drafts");
        table.add_column(column("id", "UUID", false, None));
        table
    }

    #[test]
    fn a_table_the_owner_retires_is_dropped_by_its_next_file() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("migrations")).expect("migrations");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        write_baseline(&set(
            dir.path(),
            std::slice::from_ref(&first),
            &[plain, drafts],
        ))
        .expect("a baseline");

        let gone = set(dir.path(), std::slice::from_ref(&first), &[plain]);
        assert!(
            matches!(check(&gone), Err(GenerateError::Refused { .. })),
            "a table gone without being retired is refused"
        );
        let retired = gone.retired(&["drafts"]);
        assert_eq!(retired.retired_tables(), ["drafts"]);
        match check(&retired) {
            Err(GenerateError::Pending { statements, .. }) => {
                assert_eq!(statements, ["DROP TABLE IF EXISTS drafts;"]);
            }
            other => panic!("pending, not {other:?}"),
        }
        let written = write_next(&retired, "drafts_retired").expect("written");
        let second = std::fs::read_to_string(&written).expect("the file");
        let after = set(
            dir.path(),
            &[first, ("0002_drafts_retired", second)],
            &[plain],
        )
        .retired(&["drafts"]);
        check(&after).expect("once dropped, a retired table needs nothing");
    }

    const ROLE_UNASKED: &str =
        "DO $$ BEGIN CREATE ROLE www NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;";

    fn baselined(dir: &std::path::Path, files: &[(&str, String)]) -> EmbeddedMigrations {
        std::fs::create_dir_all(dir.join("migrations")).expect("migrations");
        write_baseline(&set(dir, files, &[plain])).expect("a baseline");
        set(dir, files, &[plain])
    }

    fn timed() -> TableSchema {
        articles(|table| {
            table.add_column(column("at", "TIMESTAMPTZ", false, Some("now()")));
            table.add_index(crate::migration::TableIndex {
                name: "articles_by_at".to_owned(),
                columns: Vec::new(),
                unique: false,
                condition: None,
                key: crate::migration::KeyPart::parts(&[(
                    "at",
                    Some(fundamental_core::model::sql::ShardKind::Desc),
                )]),
                include: Vec::new(),
                constraint: false,
            });
        })
    }

    fn timed_and_accepted() -> TableSchema {
        let mut table = timed();
        table.accept(
            crate::migration::Lint::HotShard,
            "read newest first, written once a minute",
        );
        table
    }

    fn accepting_a_backfill() -> TableSchema {
        let mut table = plain();
        table.accept(
            crate::migration::Lint::Backfill,
            "the importer fills it once",
        );
        table
    }

    fn created(table: &TableSchema) -> String {
        generated(
            &crate::migration::render::create_all(table)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_lint_that_fires_in_an_owners_file_fails_the_check_until_its_model_accepts_it() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [("0001_articles", created(&timed()))];

        match check(&baselined_over(dir.path(), &files, &[timed])) {
            Err(GenerateError::Linted { findings, .. }) => {
                assert_eq!(findings.len(), 1, "{findings:?}");
                assert_eq!(findings[0].lint, crate::migration::Lint::HotShard);
                assert_eq!(
                    findings[0].migration.as_deref(),
                    Some("articles/0001_articles")
                );
            }
            other => panic!("refused by the lint, not {other:?}"),
        }
        let accepted = tempfile::tempdir().expect("a directory");
        check(&baselined_over(
            accepted.path(),
            &files,
            &[timed_and_accepted],
        ))
        .expect("the model accepts it");
    }

    #[test]
    fn an_acceptance_whose_lint_fires_nowhere_fails_the_check() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [("0001_articles", created(&plain()))];

        match check(&baselined_over(dir.path(), &files, &[accepting_a_backfill])) {
            Err(GenerateError::Linted { findings, .. }) => {
                assert!(
                    findings[0].at.contains("fires on it nowhere"),
                    "{findings:?}"
                );
            }
            other => panic!("refused as stale, not {other:?}"),
        }
    }

    #[test]
    fn the_next_file_is_not_written_when_the_lints_refuse_it() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [("0001_articles", created(&plain()))];
        std::fs::create_dir_all(dir.path().join("migrations")).expect("migrations");
        write_baseline(&set(dir.path(), &files, &[plain])).expect("a baseline");

        let refused = write_next(&set(dir.path(), &files, &[timed]), "at").expect_err("refused");

        assert!(
            matches!(refused, GenerateError::Linted { .. }),
            "{refused:?}"
        );
        assert!(!dir.path().join("migrations/sql/0002_at.up.sql").exists());
    }

    #[test]
    fn a_file_that_makes_a_role_without_looking_it_up_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            ("0002_www_reads", ROLE_UNASKED.to_owned()),
        ];

        match check(&baselined(dir.path(), &files)) {
            Err(GenerateError::RoleNotLookedUp {
                migration, role, ..
            }) => {
                assert_eq!((migration, role.as_str()), ("0002_www_reads", "www"));
            }
            other => panic!("refused for its role, not {other:?}"),
        }
    }

    #[test]
    fn a_file_the_owner_says_needs_createrole_passes() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            ("0002_www_reads", ROLE_UNASKED.to_owned()),
        ];

        let owned = baselined(dir.path(), &files).needing_createrole(&["0002_www_reads"]);

        assert_eq!(owned.files_needing_createrole(), ["0002_www_reads"]);
        check(&owned).expect("the owner says which file a migrator runs only with CREATEROLE");
    }

    #[test]
    fn a_file_said_to_need_createrole_that_does_not_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [("0001_articles", "CREATE TABLE articles (...);".to_owned())];

        let owned = baselined(dir.path(), &files).needing_createrole(&["0001_articles"]);

        match check(&owned) {
            Err(GenerateError::NeedsNoCreaterole { migration, .. }) => {
                assert_eq!(migration, "0001_articles");
            }
            other => panic!("refused for what it says, not {other:?}"),
        }
    }

    fn retiring() -> TableSchema {
        articles(|table| {
            table.retired.columns = vec!["summary".to_owned()];
            table
                .renamed
                .insert("headline".to_owned(), "title".to_owned());
        })
    }

    fn baselined_over(
        dir: &std::path::Path,
        files: &[(&str, String)],
        tables: &'static [fn() -> TableSchema],
    ) -> EmbeddedMigrations {
        std::fs::create_dir_all(dir.join("migrations")).expect("migrations");
        write_baseline(&set(dir, files, tables)).expect("a baseline");
        set(dir, files, tables)
    }

    #[test]
    fn what_the_models_retire_rename_or_the_owner_retired_passes_the_check() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_articles_reshaped",
                generated(&[
                    "ALTER TABLE articles RENAME COLUMN title TO headline;",
                    "ALTER TABLE articles DROP COLUMN IF EXISTS summary;",
                    "DROP TABLE IF EXISTS drafts;",
                ]),
            ),
        ];

        check(&baselined_over(dir.path(), &files, &[retiring]).retired(&["drafts"]))
            .expect("each drop and rename is declared");
    }

    #[test]
    fn a_retirement_taken_off_after_its_drop_fails_the_check() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_articles_summary",
                generated(&["ALTER TABLE articles DROP COLUMN IF EXISTS summary;"]),
            ),
        ];

        let error = check(&baselined_over(dir.path(), &files, &[plain])).expect_err("undeclared");
        match &error {
            GenerateError::UndeclaredDrop {
                migration,
                verb,
                object,
                ..
            } => assert_eq!(
                (*migration, verb.as_str(), object.as_str()),
                ("0002_articles_summary", "DROP COLUMN", "articles.summary")
            ),
            other => panic!("an undeclared drop, not {other:?}"),
        }
        let said = error.to_string();
        assert!(
            said.contains("drops_by_hand = [\"0002_articles_summary\"]") && said.contains("retire"),
            "{said}"
        );
    }

    #[test]
    fn a_drop_written_by_hand_passes_only_when_its_file_is_named() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_articles_featured",
                "ALTER TABLE articles DROP COLUMN IF EXISTS featured;".to_owned(),
            ),
        ];
        let owner = baselined_over(dir.path(), &files, &[plain]);

        assert!(matches!(
            check(&owner),
            Err(GenerateError::UndeclaredDrop { .. })
        ));
        let named = owner.dropping_by_hand(&["0002_articles_featured"]);
        check(&named).expect("the owner names the file");
    }

    #[test]
    fn a_reader_taken_off_the_model_passes_the_check_in_its_generated_file() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_www_reads_no_more",
                generated(&[
                    "REVOKE SELECT ON articles FROM www;",
                    "DROP POLICY IF EXISTS articles_read_by_www ON articles;",
                    "DROP POLICY IF EXISTS articles_written ON articles;",
                    "ALTER TABLE articles DISABLE ROW LEVEL SECURITY;",
                ]),
            ),
        ];

        check(&baselined_over(dir.path(), &files, &[plain]))
            .expect("the model reads by no role, as the file says");
    }

    #[test]
    fn a_file_named_to_drop_by_hand_that_needs_no_naming_fails_the_check() {
        let dir = tempfile::tempdir().expect("a directory");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_articles_summary",
                generated(&["ALTER TABLE articles DROP COLUMN IF EXISTS summary;"]),
            ),
        ];
        let owner = baselined_over(dir.path(), &files, &[retiring]);

        for named in ["0002_articles_summary", "0001_articles", "0009_gone"] {
            let names: &'static [&'static str] = Box::leak(Box::new([named]));
            match check(&owner.dropping_by_hand(names)) {
                Err(GenerateError::DropsNothingByHand { migration, .. }) => {
                    assert_eq!(migration, named);
                }
                other => panic!("{named}: refused for what it says, not {other:?}"),
            }
        }
    }

    const ROLE_LOOKED_UP: &str = "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE \
        rolname = 'www') THEN CREATE ROLE www NOLOGIN; END IF; EXCEPTION WHEN duplicate_object \
        THEN NULL; END $$;";

    fn generated(statements: &[&str]) -> String {
        format!(
            "-- Generated from the models by `fun migrate generate`.\n\n{}\n",
            statements.join("\n\n")
        )
    }

    #[test]
    fn regenerating_an_owners_files_makes_each_role_only_where_pg_roles_lacks_it() {
        let dir = tempfile::tempdir().expect("a directory");
        let grant = "GRANT SELECT ON articles TO www;";
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            ("0002_www_reads", generated(&[ROLE_UNASKED, grant])),
            ("0003_www_reads_more", generated(&[grant, ROLE_UNASKED])),
        ];
        let owned = baselined(dir.path(), &files)
            .needing_createrole(&["0002_www_reads", "0003_www_reads_more"]);

        let written = regenerate(&owned).expect("regenerated");

        let sql = dir.path().join("migrations/sql");
        assert_eq!(
            written,
            [
                sql.join("0002_www_reads.up.sql"),
                sql.join("0003_www_reads_more.up.sql")
            ]
        );
        let read = |name: &str| std::fs::read_to_string(sql.join(name)).expect("written");
        assert_eq!(
            read("0002_www_reads.up.sql"),
            generated(&[ROLE_LOOKED_UP, grant])
        );
        assert_eq!(
            read("0003_www_reads_more.up.sql"),
            generated(&[grant, ROLE_LOOKED_UP])
        );
        let rewritten = [
            files[0].clone(),
            ("0002_www_reads", read("0002_www_reads.up.sql")),
            ("0003_www_reads_more", read("0003_www_reads_more.up.sql")),
        ];
        check(&set(dir.path(), &rewritten, &[plain]))
            .expect("every role looked up, and the snapshot after the rewritten last file");
    }

    #[test]
    fn regenerating_leaves_what_the_generator_writes_as_it_is_and_what_it_did_not_write() {
        let dir = tempfile::tempdir().expect("a directory");
        let by_hand = "CREATE ROLE reporter LOGIN PASSWORD 'secret';";
        let files = [
            (
                "0001_articles",
                generated(&["CREATE TABLE articles (...);"]),
            ),
            ("0002_reporter", by_hand.to_owned()),
            ("0003_www_reads", generated(&[ROLE_LOOKED_UP])),
        ];
        let owned = baselined(dir.path(), &files).needing_createrole(&["0002_reporter"]);

        assert_eq!(
            regenerate(&owned).expect("regenerated"),
            Vec::<std::path::PathBuf>::new()
        );
        assert!(
            !dir.path().join("migrations/sql").exists(),
            "nothing written"
        );
        check(&owned).expect("the snapshot left as it was");
    }

    #[test]
    fn regenerating_rewrites_only_the_statement_and_never_a_comment_quoting_it() {
        let dir = tempfile::tempdir().expect("a directory");
        let quoted = format!("-- Was: {ROLE_UNASKED}\n/* and once {ROLE_UNASKED} */\n");
        let files = [
            ("0001_articles", "CREATE TABLE articles (...);".to_owned()),
            (
                "0002_www_reads",
                format!("{quoted}{}", generated(&[ROLE_UNASKED])),
            ),
        ];
        let owned = baselined(dir.path(), &files).needing_createrole(&["0002_www_reads"]);

        regenerate(&owned).expect("regenerated");

        let read = std::fs::read_to_string(dir.path().join("migrations/sql/0002_www_reads.up.sql"))
            .expect("written");
        assert_eq!(read, format!("{quoted}{}", generated(&[ROLE_LOOKED_UP])));
    }

    #[test]
    fn regenerating_is_asked_for_in_its_own_variable_so_no_migration_name_asks_for_it() {
        assert_eq!(Generation::of(None, None, None), Ok(Generation::Check));
        assert_eq!(Generation::of(None, Some(""), None), Ok(Generation::Check));
        assert_eq!(
            Generation::of(None, Some("regenerate"), None),
            Ok(Generation::Next("regenerate"))
        );
        assert_eq!(
            Generation::of(None, Some("baseline"), None),
            Ok(Generation::Baseline)
        );
        assert_eq!(
            Generation::of(Some("regenerate"), Some("article_standfirst"), None),
            Ok(Generation::Regenerate)
        );
        assert_eq!(
            Generation::of(Some(""), Some("article_standfirst"), None),
            Ok(Generation::Next("article_standfirst"))
        );
    }

    #[test]
    fn an_owner_whose_snapshot_is_not_after_its_files_is_not_regenerated() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        baselined(dir.path(), std::slice::from_ref(&first));
        let added = [first, ("0002_www_reads", generated(&[ROLE_UNASKED]))];

        match regenerate(&set(dir.path(), &added, &[plain])) {
            Err(GenerateError::Stale { .. }) => {}
            other => panic!("refused as stale, not {other:?}"),
        }
    }

    fn keyed() -> TableSchema {
        articles(|table| {
            table.set_primary_key(vec!["organisation_id".to_owned(), "id".to_owned()]);
        })
    }

    fn keyed_translations() -> Option<TableSchema> {
        Some(keyed().translations(crate::migration::MigrationDatabase::SqlDb))
    }

    #[test]
    fn a_models_companion_table_is_generated_with_it() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("migrations")).expect("migrations");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        write_baseline(&set(dir.path(), std::slice::from_ref(&first), &[keyed]))
            .expect("a baseline");

        let localized = set(dir.path(), std::slice::from_ref(&first), &[keyed])
            .companions(&[keyed_translations]);
        assert_eq!(
            localized
                .schemas()
                .iter()
                .map(|table| table.name.as_str())
                .collect::<Vec<_>>(),
            ["articles", "articles_i18n"]
        );
        match check(&localized) {
            Err(GenerateError::Pending { statements, .. }) => assert!(
                statements.iter().any(|statement| statement
                    .contains("CREATE TABLE IF NOT EXISTS articles_i18n")
                    && statement.contains(
                        "PRIMARY KEY (organisation_id HASH, id ASC, field ASC, locale ASC)"
                    )),
                "{statements:?}"
            ),
            other => panic!("pending, not {other:?}"),
        }
    }
    fn with_summary() -> TableSchema {
        articles(|table| table.add_column(column("summary", "TEXT", true, None)))
    }

    fn with_both() -> TableSchema {
        articles(|table| {
            table.add_column(column("summary", "TEXT", true, None));
            table.add_column(column("standfirst", "TEXT", false, Some("''")));
        })
    }

    const SUMMARY: &str = "ALTER TABLE articles ADD COLUMN IF NOT EXISTS summary TEXT;";
    const STANDFIRST: &str =
        "ALTER TABLE articles ADD COLUMN IF NOT EXISTS standfirst TEXT NOT NULL DEFAULT '';";

    fn main_with_summary(first: &(&str, String)) -> (Main, (&'static str, String)) {
        let gained = ("0002_article_summary", generated(&[SUMMARY]));
        let snapshot = Snapshot::of(&[with_summary()], Some(content_hash(&gained.1))).to_file();
        let main = Main {
            files: [first.clone(), gained.clone()]
                .into_iter()
                .map(|(name, up)| (name.to_owned(), up))
                .collect(),
            snapshot,
        };
        (main, gained)
    }

    fn branch_tree(dir: &std::path::Path, first: &(&str, String)) {
        std::fs::create_dir_all(dir.join("migrations/sql")).expect("migrations");
        write_baseline(&set(dir, std::slice::from_ref(first), &[plain])).expect("a baseline");
    }

    fn on_disk(dir: &std::path::Path, files: &[(&str, String)]) {
        for (name, up) in files {
            std::fs::write(dir.join(format!("migrations/sql/{name}.up.sql")), up).expect("file");
        }
    }

    #[test]
    fn a_branch_file_is_written_again_on_top_of_main_with_the_next_number() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (main, gained) = main_with_summary(&first);
        let own = ("0002_article_standfirst", generated(&[STANDFIRST]));
        let merged = [first.clone(), own.clone(), gained.clone()];
        on_disk(dir.path(), &merged);

        let rebased = rebase(&set(dir.path(), &merged, &[with_both]), &main).expect("rebased");

        let sql = dir.path().join("migrations/sql");
        assert_eq!(
            rebased.removed,
            [sql.join("0002_article_standfirst.up.sql")]
        );
        assert_eq!(
            rebased.written,
            Some(sql.join("0003_article_standfirst.up.sql"))
        );
        assert!(!sql.join("0002_article_standfirst.up.sql").exists());
        let third =
            std::fs::read_to_string(sql.join("0003_article_standfirst.up.sql")).expect("written");
        assert_eq!(third, generated(&[STANDFIRST]));
        check(&set(
            dir.path(),
            &[first, gained, ("0003_article_standfirst", third)],
            &[with_both],
        ))
        .expect("the rebased file and its snapshot are the models, on top of main");
    }

    #[test]
    fn a_branch_file_main_already_made_is_removed_and_nothing_written() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (main, gained) = main_with_summary(&first);
        let own = ("0002_summary_too", generated(&[SUMMARY]));
        let merged = [first.clone(), gained.clone(), own];
        on_disk(dir.path(), &merged);

        let rebased = rebase(&set(dir.path(), &merged, &[with_summary]), &main).expect("rebased");

        assert_eq!(
            rebased.removed,
            [dir.path().join("migrations/sql/0002_summary_too.up.sql")]
        );
        assert_eq!(rebased.written, None);
        check(&set(dir.path(), &[first, gained], &[with_summary]))
            .expect("main's snapshot, for main's files");
    }

    #[test]
    fn several_branch_files_become_one_named_after_the_newest() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (main, gained) = main_with_summary(&first);
        let merged = [
            first.clone(),
            gained.clone(),
            ("0002_standfirst_draft", generated(&[STANDFIRST])),
            ("0003_standfirst_final", generated(&["SELECT 1;"])),
        ];
        on_disk(dir.path(), &merged);

        let rebased = rebase(&set(dir.path(), &merged, &[with_both]), &main).expect("rebased");

        let sql = dir.path().join("migrations/sql");
        assert_eq!(rebased.removed, [sql.join("0002_standfirst_draft.up.sql")]);
        assert_eq!(
            rebased.written,
            Some(sql.join("0003_standfirst_final.up.sql"))
        );
        assert_eq!(
            std::fs::read_to_string(sql.join("0003_standfirst_final.up.sql")).expect("written"),
            generated(&[STANDFIRST])
        );
    }

    #[test]
    fn a_main_file_the_tree_lacks_or_changed_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (main, gained) = main_with_summary(&first);
        let own = ("0002_article_standfirst", generated(&[STANDFIRST]));

        let unmerged = set(dir.path(), &[first.clone(), own.clone()], &[with_both]);
        match rebase(&unmerged, &main) {
            Err(error @ GenerateError::MainFileMissing { .. }) => {
                assert!(
                    error.to_string().contains("git merge origin/main"),
                    "{error}"
                );
            }
            other => panic!("main's file missing, not {other:?}"),
        }
        let changed = (gained.0, generated(&["SELECT 2;"]));
        let edited = set(dir.path(), &[first, own, changed], &[with_both]);
        assert!(
            matches!(
                rebase(&edited, &main),
                Err(GenerateError::MainFileChanged { migration, .. }) if migration == "0002_article_summary"
            ),
            "a main file changed in the tree is refused"
        );
    }

    #[test]
    fn a_branch_file_written_by_hand_is_refused_so_it_is_never_lost() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (main, gained) = main_with_summary(&first);
        let by_hand = (
            "0002_backfill",
            "UPDATE articles SET headline = '';".to_owned(),
        );
        let merged = [first, gained, by_hand];
        on_disk(dir.path(), &merged);

        match rebase(&set(dir.path(), &merged, &[with_summary]), &main) {
            Err(GenerateError::WrittenByHand { migration, .. }) => {
                assert_eq!(migration, "0002_backfill");
            }
            other => panic!("written by hand, not {other:?}"),
        }
        assert!(
            dir.path()
                .join("migrations/sql/0002_backfill.up.sql")
                .exists(),
            "kept"
        );
    }

    #[test]
    fn a_main_whose_snapshot_is_not_after_its_files_is_refused() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(dir.path(), &first);
        let (mut main, gained) = main_with_summary(&first);
        let merged = [first, gained];
        main.snapshot = Snapshot::of(&[with_summary()], None).to_file();
        assert!(matches!(
            rebase(&set(dir.path(), &merged, &[with_summary]), &main),
            Err(GenerateError::MainUnreadable { .. })
        ));
        main.snapshot = "<<<<<<< HEAD\n".to_owned();
        assert!(matches!(
            rebase(&set(dir.path(), &merged, &[with_summary]), &main),
            Err(GenerateError::MainUnreadable { .. })
        ));
    }
    #[test]
    fn a_stale_snapshot_names_the_rebase_that_fixes_it() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        baselined(dir.path(), std::slice::from_ref(&first));
        let added = [first, ("0002_by_hand", "SELECT 1;".to_owned())];
        let error = check(&set(dir.path(), &added, &[plain])).expect_err("stale");
        assert!(
            error
                .to_string()
                .ends_with("run `fun migrate rebase -p articles`"),
            "{error}"
        );
    }

    #[test]
    fn a_snapshot_left_conflicted_by_a_merge_is_stale_and_names_the_rebase() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        let committed = baselined(dir.path(), std::slice::from_ref(&first));
        let snapshot = committed.generation().expect("generated").snapshot;
        std::fs::write(
            dir.path().join("migrations/schema.json"),
            format!("<<<<<<< HEAD\n{snapshot}=======\n{snapshot}>>>>>>> origin/main\n"),
        )
        .expect("conflicted");
        match check(&set(dir.path(), std::slice::from_ref(&first), &[plain])) {
            Err(error @ GenerateError::Stale { .. }) => {
                assert!(error.to_string().contains("fun migrate rebase -p articles"));
            }
            other => panic!("stale, not {other:?}"),
        }
    }

    #[test]
    fn two_files_of_one_number_are_stale_whichever_snapshot_the_merge_kept() {
        let dir = tempfile::tempdir().expect("a directory");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        let theirs = ("0002_article_summary", generated(&[SUMMARY]));
        let ours = ("0002_article_standfirst", generated(&[STANDFIRST]));
        baselined(dir.path(), &[first.clone(), ours.clone(), theirs.clone()]);
        assert!(matches!(
            check(&set(dir.path(), &[first, ours, theirs], &[plain])),
            Err(GenerateError::Stale { .. })
        ));
    }
    #[test]
    fn rebasing_is_asked_for_by_its_mode_and_names_the_commit_it_is_on() {
        assert_eq!(
            Generation::of(Some("rebase"), Some("article_standfirst"), Some("3953fc6")),
            Ok(Generation::Rebase("3953fc6"))
        );
        let refused = Generation::of(Some("rebase"), None, None).expect_err("no base");
        assert!(refused.contains("FUNDAMENTAL_MIGRATION_BASE"), "{refused}");
        let refused = Generation::of(Some("rebase"), None, Some(" ")).expect_err("empty base");
        assert!(refused.contains("FUNDAMENTAL_MIGRATION_BASE"), "{refused}");
        let refused = Generation::of(Some("squash"), None, None).expect_err("no such mode");
        assert!(refused.contains("`regenerate` or `rebase`"), "{refused}");
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_branch_is_rebased_on_main_as_the_commit_holds_it() {
        let repository = tempfile::tempdir().expect("a directory");
        let dir = repository.path().join("domain/articles");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(&dir, &first);
        let (main, gained) = main_with_summary(&first);
        on_disk(&dir, &[first.clone(), gained.clone()]);
        std::fs::write(dir.join("migrations/schema.json"), &main.snapshot).expect("main's");
        git(repository.path(), &["init", "-q", "-b", "main"]);
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-q", "-m", "main"]);
        let own = ("0002_article_standfirst", generated(&[STANDFIRST]));
        on_disk(&dir, std::slice::from_ref(&own));
        std::fs::write(dir.join("migrations/schema.json"), "<<<<<<< HEAD\n").expect("conflicted");
        let merged = [first, own, gained];

        let rebased = rebase_on(&set(&dir, &merged, &[with_both]), "main").expect("rebased");

        assert_eq!(
            rebased.written,
            Some(dir.join("migrations/sql/0003_article_standfirst.up.sql"))
        );
        let refused = rebase_on(&set(&dir, &merged, &[with_both]), "no-such-branch");
        assert!(
            matches!(refused, Err(GenerateError::MainUnreadable { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn an_owner_main_does_not_hold_yet_is_left_as_it_is() {
        let repository = tempfile::tempdir().expect("a directory");
        std::fs::write(repository.path().join("README"), "main\n").expect("main's file");
        git(repository.path(), &["init", "-q", "-b", "main"]);
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-q", "-m", "main"]);
        let dir = repository.path().join("domain/articles");
        let first = ("0001_articles", "CREATE TABLE articles (...);".to_owned());
        branch_tree(&dir, &first);
        on_disk(&dir, std::slice::from_ref(&first));
        let before = std::fs::read_to_string(dir.join("migrations/schema.json")).expect("ours");

        let rebased = rebase_on(&set(&dir, std::slice::from_ref(&first), &[plain]), "main")
            .expect("nothing on main to rebase on");

        assert_eq!(rebased, crate::migration::generate::Rebased::default());
        assert!(dir.join("migrations/sql/0001_articles.up.sql").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("migrations/schema.json")).expect("kept"),
            before
        );
    }
}

mod readers {
    use super::{articles, next_statements, snapshot};
    use crate::migration::TableSchema;
    use crate::migration::ddl::{DdlStatement, recognise};

    fn read_by_the_site() -> TableSchema {
        articles(|table| table.add_reader("www", "state = 'published'"))
    }

    const POLICIES: [&str; 5] = [
        "ALTER TABLE articles ENABLE ROW LEVEL SECURITY;",
        "DO $$ BEGIN CREATE POLICY articles_written ON articles USING (true) WITH CHECK (true); \
         EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'www') THEN \
         CREATE ROLE www NOLOGIN; END IF; EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
        "DO $$ BEGIN CREATE POLICY articles_read_by_www ON articles AS RESTRICTIVE FOR SELECT \
         TO www USING (state = 'published'); EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
        "GRANT SELECT ON articles TO www;",
    ];

    #[test]
    fn a_table_a_role_reads_lets_it_see_only_the_rows_it_names() {
        let statements =
            next_statements(&snapshot(&[]), &snapshot(&[read_by_the_site()])).expect("written");
        assert!(statements[0].starts_with("CREATE TABLE IF NOT EXISTS articles ("));
        assert_eq!(statements[1..], POLICIES);
    }

    #[test]
    fn a_readers_role_is_made_only_where_pg_roles_lacks_it() {
        let statements =
            next_statements(&snapshot(&[]), &snapshot(&[read_by_the_site()])).expect("written");
        let made: Vec<_> = statements
            .iter()
            .filter_map(|statement| match recognise(statement) {
                Ok(DdlStatement::CreateRole { role, looked_up }) => Some((role, looked_up)),
                _ => None,
            })
            .collect();
        assert_eq!(made, [("www".to_owned(), true)]);
    }

    #[test]
    fn a_reader_given_to_a_table_that_has_rows_is_added_to_it() {
        let statements = next_statements(
            &snapshot(&[articles(|_| {})]),
            &snapshot(&[read_by_the_site()]),
        )
        .expect("written");
        assert_eq!(statements, POLICIES);
        let both = snapshot(&[read_by_the_site()]);
        assert_eq!(next_statements(&both, &both), Ok(Vec::new()));
    }

    #[test]
    fn a_reader_whose_rows_change_or_who_goes_is_told_so() {
        let wider = articles(|table| {
            table.add_reader("www", "state IN ('published', 'archived')");
        });
        assert_eq!(
            next_statements(&snapshot(&[read_by_the_site()]), &snapshot(&[wider])),
            Ok(vec![
                "ALTER POLICY articles_read_by_www ON articles \
                 USING (state IN ('published', 'archived'));"
                    .to_owned()
            ])
        );
        assert_eq!(
            next_statements(
                &snapshot(&[read_by_the_site()]),
                &snapshot(&[articles(|_| {})])
            ),
            Ok(vec![
                "REVOKE SELECT ON articles FROM www;".to_owned(),
                "DROP POLICY IF EXISTS articles_read_by_www ON articles;".to_owned(),
                "DROP POLICY IF EXISTS articles_written ON articles;".to_owned(),
                "ALTER TABLE articles DISABLE ROW LEVEL SECURITY;".to_owned(),
            ])
        );
    }

    #[test]
    fn a_tables_translations_are_read_with_the_rows_their_reader_sees() {
        let mut table = read_by_the_site();
        table.set_primary_key(vec!["organisation_id".to_owned(), "id".to_owned()]);
        let companion = table.translations(crate::migration::MigrationDatabase::SqlDb);
        assert_eq!(
            companion.readers.get("www").map(String::as_str),
            Some(
                "EXISTS (SELECT 1 FROM articles WHERE articles.organisation_id = \
                 articles_i18n.organisation_id AND articles.id = articles_i18n.id)"
            )
        );
    }
}

mod placement {
    use super::{articles, next_statements, snapshot};
    use crate::migration::Lints;

    #[test]
    fn a_table_out_of_colocation_says_so_only_in_a_colocated_database() {
        let declared = snapshot(&[articles(|table| {
            table.set_colocated(false);
            table.set_tablets(8);
        })]);

        let colocated = super::super::next_statements(
            &snapshot(&[]),
            &declared,
            &Lints::default().colocated(true),
        )
        .expect("written");
        let apart = next_statements(&snapshot(&[]), &declared).expect("written");

        assert!(
            colocated[0].ends_with(") WITH (COLOCATION = false) SPLIT INTO 8 TABLETS;"),
            "{}",
            colocated[0]
        );
        assert!(
            apart[0].ends_with(") SPLIT INTO 8 TABLETS;"),
            "{}",
            apart[0]
        );
    }

    #[test]
    fn a_colocated_table_with_a_hashed_key_is_refused_in_a_colocated_database() {
        let colocated = Lints::default().colocated(true);
        let range = articles(|table| {
            table.set_key(crate::migration::KeyPart::parts(&[
                ("organisation_id", Some(super::ShardKind::Asc)),
                ("id", Some(super::ShardKind::Asc)),
            ]));
        });

        let refused = super::super::next_statements(
            &snapshot(&[]),
            &snapshot(&[articles(|_| {})]),
            &colocated,
        )
        .expect_err("refused");
        let apart = articles(|table| table.set_colocated(false));

        assert!(
            refused[0]
                .reason
                .contains("cannot colocate hash partitioned table")
        );
        super::super::next_statements(&snapshot(&[]), &snapshot(&[range]), &colocated)
            .expect("a range key colocates");
        super::super::next_statements(&snapshot(&[]), &snapshot(&[apart]), &colocated)
            .expect("a table apart hashes");
    }

    #[test]
    fn colocation_or_tablets_changed_on_a_table_that_exists_is_refused_as_a_rewrite() {
        let before = snapshot(&[articles(|_| {})]);
        for changed in [
            articles(|table| table.set_colocated(false)),
            articles(|table| table.set_tablets(4)),
        ] {
            let refused = next_statements(&before, &snapshot(&[changed])).expect_err("refused");
            assert!(
                refused[0].reason.starts_with("MIG_L02 table_rewrite:"),
                "{}",
                refused[0].reason
            );
        }
    }

    #[test]
    fn a_new_writer_is_granted_and_one_gone_is_revoked() {
        let read = articles(|_| {});
        let written = articles(|table| table.add_writer("importer"));

        let granted = next_statements(
            &snapshot(std::slice::from_ref(&read)),
            &snapshot(std::slice::from_ref(&written)),
        )
        .expect("written");
        let revoked = next_statements(
            &snapshot(std::slice::from_ref(&written)),
            &snapshot(&[read]),
        )
        .expect("written");

        assert_eq!(
            granted.last().map(String::as_str),
            Some("GRANT SELECT, INSERT, UPDATE, DELETE ON articles TO importer;")
        );
        assert_eq!(
            revoked,
            ["REVOKE SELECT, INSERT, UPDATE, DELETE ON articles FROM importer;"]
        );
        let both = snapshot(&[written]);
        assert_eq!(next_statements(&both, &both), Ok(Vec::new()));
    }

    #[test]
    fn placement_and_writers_are_kept_in_the_snapshot_and_acceptances_are_not() {
        let table = articles(|table| {
            table.set_colocated(false);
            table.set_tablets(8);
            table.add_writer("importer");
            table.accept(
                crate::migration::Lint::HotShard,
                "one writer, once a second",
            );
        });

        let file = snapshot(&[table]).to_file();
        let read = crate::migration::generate::Snapshot::from_file(&file).expect("read");

        let kept = &read.tables["articles"];
        assert_eq!(kept.colocated, Some(false));
        assert_eq!(kept.tablets, Some(8));
        assert!(kept.writers.contains("importer"));
        assert!(kept.accepted.is_empty());
        assert!(!file.contains("hot_shard"));
    }
}

mod column_changes {
    use super::{articles, column, next_statements, snapshot};
    use crate::migration::{Level, Lint, Lints};

    fn total(data_type: &str) -> crate::migration::TableSchema {
        articles(|table| table.add_column(column("total", data_type, false, Some("0"))))
    }

    #[test]
    fn a_narrowed_column_is_refused_naming_both_lints_by_default() {
        let refused = next_statements(&snapshot(&[total("INT8")]), &snapshot(&[total("INT4")]))
            .expect_err("refused");

        assert!(
            refused[0]
                .reason
                .starts_with("MIG_L02 table_rewrite, MIG_L10 narrowed_type: column `total`"),
            "{}",
            refused[0].reason
        );
    }

    #[test]
    fn a_retype_the_project_warns_of_and_the_model_accepts_is_written() {
        let lints = Lints::default()
            .with(Lint::TableRewrite, Level::Warn)
            .with(Lint::NarrowedType, Level::Warn);
        let mut accepted = total("INT4");
        accepted.accept(Lint::TableRewrite, "the table holds a few hundred rows");
        accepted.accept(Lint::NarrowedType, "no total has passed a million");

        let written = super::super::next_statements(
            &snapshot(&[total("INT8")]),
            &snapshot(&[accepted]),
            &lints,
        )
        .expect("written");
        let unaccepted = super::super::next_statements(
            &snapshot(&[total("INT8")]),
            &snapshot(&[total("INT4")]),
            &lints,
        );

        assert_eq!(
            written,
            ["ALTER TABLE articles ALTER COLUMN total TYPE INT4;"]
        );
        assert!(unaccepted.is_err());
    }

    #[test]
    fn a_column_made_required_is_set_not_null_once_the_lint_passes() {
        let optional = articles(|table| table.add_column(column("note", "TEXT", true, None)));
        let required = articles(|table| table.add_column(column("note", "TEXT", false, None)));
        let allowing = Lints::default().with(Lint::NotNullWithoutDefault, Level::Allow);

        let refused = next_statements(
            &snapshot(std::slice::from_ref(&optional)),
            &snapshot(std::slice::from_ref(&required)),
        )
        .expect_err("refused by default");
        let written = super::super::next_statements(
            &snapshot(&[optional]),
            &snapshot(&[required]),
            &allowing,
        )
        .expect("written");

        assert!(
            refused[0]
                .reason
                .starts_with("MIG_L09 not_null_without_default")
        );
        assert_eq!(
            written,
            ["ALTER TABLE articles ALTER COLUMN note SET NOT NULL;"]
        );
    }

    #[test]
    fn a_changed_key_is_refused_as_a_changed_split() {
        let rekeyed = articles(|table| {
            table.set_key(crate::migration::KeyPart::parts(&[(
                "id",
                Some(fundamental_core::model::sql::ShardKind::Hash),
            )]));
        });

        let refused = next_statements(&snapshot(&[articles(|_| {})]), &snapshot(&[rekeyed]))
            .expect_err("refused");

        assert!(
            refused[0].reason.starts_with("MIG_L03 key_split_changed:"),
            "{}",
            refused[0].reason
        );
    }
}
