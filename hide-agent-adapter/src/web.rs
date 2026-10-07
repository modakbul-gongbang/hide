//! The generated shell contract is a projection, never another registry.

#[derive(serde::Serialize)]
pub struct WebAdapter {
    pub id: &'static str,
    pub label: &'static str,
    pub picker_label: &'static str,
    pub herdr_kind: &'static str,
    pub doc_url: &'static str,
    pub install_url: &'static str,
    pub logo_id: &'static str,
    pub can_start: bool,
}

pub fn web_contract() -> impl Iterator<Item = WebAdapter> {
    super::ADAPTERS.iter().map(|row| WebAdapter {
        id: row.id,
        label: row.label,
        picker_label: row.picker_label,
        herdr_kind: row.herdr.name,
        doc_url: row.doc_url,
        install_url: row.install_url,
        logo_id: row.logo_id,
        can_start: row.start.is_some(),
    })
}
