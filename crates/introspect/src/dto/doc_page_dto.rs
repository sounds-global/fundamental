use serde::{Deserialize, Serialize};

/// One authored page as fetched surface data; mirrors [`DocPage`](crate::DocPage).
///
/// The `body` comes from the serving app, so a consumer renders it as untrusted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocPageDto {
    /// The page's title.
    pub title: String,
    /// The sidebar group the page files under.
    pub group: String,
    /// The page's sort order within its group (lower renders first).
    pub order: i32,
    /// The page's HTML body — authored by the serving app, untrusted by consumers.
    pub body: String,
}

#[cfg(feature = "ssr")]
impl From<&crate::DocPage> for DocPageDto {
    fn from(page: &crate::DocPage) -> Self {
        Self {
            title: page.title().to_owned(),
            group: page.group_name().to_owned(),
            order: page.sort_order(),
            body: page.body().to_owned(),
        }
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use crate::DocPage;

    #[test]
    fn converts_and_round_trips() {
        let dto = DocPageDto::from(
            &DocPage::new("Overview", "<p>Hi</p>")
                .group("Guides")
                .order(3),
        );
        assert_eq!(dto.title, "Overview");
        assert_eq!(dto.group, "Guides");
        assert_eq!(dto.order, 3);
        assert_eq!(dto.body, "<p>Hi</p>");

        let json = serde_json::to_string(&dto).expect("serialize");
        assert_eq!(serde_json::from_str::<DocPageDto>(&json).unwrap(), dto);
    }
}
