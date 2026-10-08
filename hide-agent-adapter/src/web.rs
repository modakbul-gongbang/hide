//! The generated shell contract is a projection, never another registry.

#[derive(serde::Serialize)]
pub struct WebAdapter {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub label: &'static str,
    pub picker_label: &'static str,
    pub herdr_kind: &'static str,
    pub doc_url: &'static str,
    pub install_url: &'static str,
    pub logo_id: &'static str,
    pub can_start: bool,
    pub can_resume: bool,
    pub sidebar_mark: Option<super::SidebarMark>,
    /// What Settings calls the agent's hook piece: a hook entry or a plugin
    /// file; absent for an agent Hide writes no hook for.
    pub hook_kind: Option<HookKind>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Hook,
    Plugin,
}

pub fn web_contract() -> impl Iterator<Item = WebAdapter> {
    super::ADAPTERS.iter().map(|row| WebAdapter {
        id: row.id,
        aliases: row.aliases,
        label: row.label,
        picker_label: row.picker_label,
        herdr_kind: row.herdr.name,
        doc_url: row.doc_url,
        install_url: row.install_url,
        logo_id: row.logo_id,
        can_start: row.start.is_some(),
        can_resume: row.resume.is_some(),
        sidebar_mark: row.sidebar_mark,
        hook_kind: match row.hook {
            super::HookInstall::Runtime(_) | super::HookInstall::Guidance(_) => {
                Some(HookKind::Hook)
            }
            super::HookInstall::Plugin(_) => Some(HookKind::Plugin),
            super::HookInstall::None => None,
        },
    })
}
