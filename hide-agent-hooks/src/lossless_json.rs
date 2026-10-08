//! Source-preserving edits using serde_json's parser, not another JSON lexer.
//!
//! Unchanged members and array entries keep their literal source. A changed
//! hook group is matched by its surviving handlers so mixed ownership also
//! preserves the operator's command spelling, whitespace and escaping.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, Visitor};
use serde_json::{Value, value::RawValue};

pub(crate) fn parse(source: &str) -> Result<Value, serde_json::Error> {
    // This also enforces serde_json's nesting limit before traversing raw
    // children. RawValue alone deliberately does not enforce that limit.
    let value = serde_json::from_str(source)?;
    Node::parse(source, &value)?;
    Ok(value)
}

pub(crate) fn rewrite(source: &str, target: &Value) -> Result<String, serde_json::Error> {
    let value = serde_json::from_str(source)?;
    let node = Node::parse(source, &value)?;
    let start = node.source.as_ptr() as usize - source.as_ptr() as usize;
    let mut rendered = source[..start].to_owned();
    rendered.push_str(&node.render(&value, target)?);
    rendered.push_str(&source[start + node.source.len()..]);
    Ok(rendered)
}

struct Node<'a> {
    source: &'a str,
    entries: Vec<Entry<'a>>,
}

struct Entry<'a> {
    key: Option<String>,
    start: usize,
    value_start: usize,
    end: usize,
    leading_start: usize,
    node: Node<'a>,
}

/// MapAccess retains duplicate members, unlike Value's object map. Decoded
/// names are compared, so "hooks" and "h\\u006foks" are the same key.
struct Members<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Members<'de>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object with unique member names")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                let mut names = BTreeSet::new();
                while let Some((name, raw)) = map.next_entry::<String, &RawValue>()? {
                    if !names.insert(name.clone()) {
                        return Err(de::Error::custom("duplicate JSON member name"));
                    }
                    members.push((name, raw));
                }
                Ok(Members(members))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

impl<'a> Node<'a> {
    fn parse(source: &'a str, value: &Value) -> Result<Self, serde_json::Error> {
        let raw: &RawValue = serde_json::from_str(source)?;
        let source = raw.get();
        let children = match value {
            Value::Object(_) => serde_json::from_str::<Members<'_>>(source)?
                .0
                .into_iter()
                .map(|(name, raw)| (Some(name), raw))
                .collect::<Vec<_>>(),
            Value::Array(_) => serde_json::from_str::<Vec<&RawValue>>(source)?
                .into_iter()
                .map(|raw| (None, raw))
                .collect(),
            _ => Vec::new(),
        };
        let mut entries = Vec::with_capacity(children.len());
        let mut cursor = 1;
        for (index, (key, raw)) in children.into_iter().enumerate() {
            let value_start = raw.get().as_ptr() as usize - source.as_ptr() as usize;
            let end = value_start + raw.get().len();
            if index != 0 {
                // The established parser has already checked the punctuation.
                cursor = skip_whitespace(source, cursor) + 1;
            }
            let leading_start = cursor;
            let start = skip_whitespace(source, cursor);
            let child_value = match &key {
                Some(key) => &value[key],
                None => &value[index],
            };
            entries.push(Entry {
                key,
                start,
                value_start,
                end,
                leading_start,
                node: Self::parse(raw.get(), child_value)?,
            });
            cursor = end;
        }
        Ok(Self { source, entries })
    }

    fn entry_value<'v>(&self, value: &'v Value, index: usize) -> &'v Value {
        match &self.entries[index].key {
            Some(key) => &value[key],
            None => &value[index],
        }
    }

    fn render(&self, before: &Value, target: &Value) -> Result<String, serde_json::Error> {
        if before == target {
            return Ok(self.source.to_owned());
        }
        let targets = match (before, target) {
            (Value::Object(_), Value::Object(values)) => values
                .iter()
                .map(|(key, value)| (Some(key.as_str()), value))
                .collect::<Vec<_>>(),
            (Value::Array(_), Value::Array(values)) => {
                values.iter().map(|value| (None, value)).collect()
            }
            _ => return serde_json::to_string(target),
        };
        let mut mapping = vec![None; targets.len()];
        let object_positions: BTreeMap<_, _> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.key.as_deref().map(|key| (key, index)))
            .collect();
        // Object members have unique decoded keys, so their source identity
        // does not depend on target iteration order. Array entries retain
        // order, including mixed groups that lost only marked commands.
        // Equal handlers in different groups must not swap their source spans.
        let mut next = 0;
        for (slot, (key, value)) in targets.iter().enumerate() {
            let candidate = match key {
                Some(key) => object_positions.get(key).copied(),
                None => self
                    .entries
                    .iter()
                    .enumerate()
                    .skip(next)
                    .find_map(|(index, _)| {
                        let matches = self.entry_value(before, index) == *value
                            || shares_handler(self.entry_value(before, index), value);
                        matches.then_some(index)
                    }),
            };
            if let Some(index) = candidate {
                mapping[slot] = Some(index);
                next = index + 1;
            }
        }
        let mut rendered = String::new();
        let first_start = self.entries.first().map_or(1, |entry| entry.start);
        rendered.push_str(&self.source[..first_start]);
        let mut previous: Option<usize> = None;
        for (slot, ((key, value), matched)) in targets.iter().zip(&mapping).enumerate() {
            if slot != 0 {
                match (previous, matched) {
                    (Some(previous), Some(index)) if previous + 1 == *index => {
                        rendered.push_str(
                            &self.source[self.entries[previous].end..self.entries[*index].start],
                        );
                    }
                    _ => {
                        rendered.push(',');
                        if let Some(index) = matched {
                            let entry = &self.entries[*index];
                            rendered.push_str(&self.source[entry.leading_start..entry.start]);
                        }
                    }
                }
            }
            if let Some(index) = matched {
                let entry = &self.entries[*index];
                rendered.push_str(&self.source[entry.start..entry.value_start]);
                rendered.push_str(&entry.node.render(self.entry_value(before, *index), value)?);
            } else {
                if let Some(key) = key {
                    rendered.push_str(&serde_json::to_string(key)?);
                    rendered.push(':');
                }
                rendered.push_str(&serde_json::to_string(value)?);
            }
            previous = *matched;
        }
        let tail_start = self.entries.last().map_or(1, |entry| entry.end);
        rendered.push_str(&self.source[tail_start..]);
        Ok(rendered)
    }
}

fn skip_whitespace(source: &str, mut offset: usize) -> usize {
    while source
        .as_bytes()
        .get(offset)
        .is_some_and(u8::is_ascii_whitespace)
    {
        offset += 1;
    }
    offset
}

fn shares_handler(before: &Value, after: &Value) -> bool {
    let (Some(before_group), Some(after_group)) = (before.as_object(), after.as_object()) else {
        return false;
    };
    if before_group
        .iter()
        .filter(|(key, _)| *key != "hooks")
        .ne(after_group.iter().filter(|(key, _)| *key != "hooks"))
    {
        return false;
    }
    let (Some(before), Some(after)) = (
        before.get("hooks").and_then(Value::as_array),
        after.get("hooks").and_then(Value::as_array),
    ) else {
        return false;
    };
    before.iter().any(|handler| after.contains(handler))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reordered_object_members_keep_their_original_literal_values() {
        let first = r#""a\u006cpha" : { "text":"\u2603", "number":1e+02 }"#;
        let second = r#""beta"  : [ true, null ]"#;
        let source = format!("{{{first},{second}}}");
        let mut target = super::parse(&source).unwrap();
        let members = target.as_object_mut().unwrap();
        let first_value = members.shift_remove("alpha").unwrap();
        members.insert("alpha".into(), first_value);
        // An owned edit forces a render even though the surviving values match.
        members.insert("owned".into(), serde_json::json!(true));
        let rewritten = super::rewrite(&source, &target).unwrap();
        assert!(rewritten.contains(first));
        assert!(rewritten.contains(second));
        assert!(rewritten.find(second).unwrap() < rewritten.find(first).unwrap());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rewritten).unwrap(),
            target
        );
    }
}
