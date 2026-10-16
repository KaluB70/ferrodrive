//! Small helpers over `roxmltree` for vendor catalogs.

use roxmltree::Node;

/// First descendant element with this local name (namespaces ignored).
pub fn find<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

/// All descendant elements with this local name.
pub fn all<'a, 'i: 'a>(
    node: Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.descendants()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}

/// Direct child element with this local name.
pub fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

/// Trimmed text of the first descendant with this name, if not empty.
pub fn text(node: Node<'_, '_>, name: &str) -> Option<String> {
    find(node, name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Trimmed text of a direct child with this name, if not empty.
pub fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    child(node, name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Parses already-decoded text. The XML declaration is dropped because
/// catalogs declare `utf-16` even after we've decoded them to UTF-8.
pub fn parse(text: &str) -> Result<roxmltree::Document<'_>, String> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let text = match text.strip_prefix("<?xml") {
        Some(rest) => rest.split_once("?>").map_or(rest, |(_, body)| body),
        None => text,
    };
    roxmltree::Document::parse(text).map_err(|e| format!("bad XML: {e}"))
}
