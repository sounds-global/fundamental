use serde_json::Value;

/// One node of the page's accessibility tree, as a screen reader is given it.
#[derive(Clone, Debug)]
pub struct AxNode {
    /// The computed role: `button`, `textbox`, `heading`, `status`, …
    pub role: String,
    /// The accessible name.
    pub name: String,
    /// Whether the node is left out of the tree a reader walks — hidden, or presentational.
    pub ignored: bool,
    /// Every property (`invalid`, `required`, `level`, …) as the browser reported it.
    pub properties: Vec<(String, Value)>,
}

impl AxNode {
    /// The value of the property `name`, when the node has it.
    #[must_use]
    pub fn property(&self, name: &str) -> Option<&Value> {
        self.properties
            .iter()
            .find(|(property, _)| property == name)
            .map(|(_, value)| value)
    }
}
