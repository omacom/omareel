//! The editor's overlay surface for QML (see `omareel_core::editor` overlays): the list, the
//! selection, what the composition draws, and the edits. Values from QML arrive as
//! `QVariantMap`s and become JSON with `QJsonValue::fromVariant`'s rules.

use super::{qobject, qstring};
use crate::export::json_variant;
use core::pin::Pin;
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use omareel_core::compat::qjson::{self, Object, Value};

/// A `QVariantMap` from QML as a JSON object.
fn json_object(map: &QMap<QMapPair_QString_QVariant>) -> Object {
    let json = qobject::project_value_json(&QVariant::from(map), false).to_string();
    match qjson::from_json(json.as_bytes()) {
        Ok(Value::Array(items)) => items.first().map(Value::to_object).unwrap_or_default(),
        _ => Object::new(),
    }
}

fn variant_list(items: Vec<Object>) -> QList<QVariant> {
    items.into_iter().map(|item| json_variant(&Value::Object(item))).collect()
}

impl qobject::Editor {
    pub(super) fn overlays(&self) -> QList<QVariant> {
        let Some(document) = self.document.as_ref() else { return QList::default() };
        variant_list(
            document
                .overlays()
                .iter()
                .map(|overlay| {
                    let mut json = overlay.to_json();
                    let (start, end) = document.overlay_output_range(overlay);
                    json.insert("outputStart", Value::Double(start));
                    json.insert("outputEnd", Value::Double(end));
                    json
                })
                .collect(),
        )
    }

    pub(super) fn selected_overlay_id(&self) -> QString {
        self.document.as_ref().map_or_else(QString::default, |document| qstring(document.selected_overlay_id()))
    }

    pub(super) fn set_selected_overlay_id(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.edit(|document| document.set_selected_overlay_id(&id));
    }

    pub(super) fn preview_overlays(&self) -> QList<QVariant> {
        self.document.as_ref().map_or_else(QList::default, |document| variant_list(document.preview_overlays()))
    }

    pub(super) fn add_overlay_at(self: Pin<&mut Self>, kind: &QString, output_time: f64, length: f64) -> QString {
        let kind = kind.to_string();
        qstring(&self.edit(|document| document.add_overlay_at(&kind, output_time, length)).unwrap_or_default())
    }

    pub(super) fn add_overlay(
        self: Pin<&mut Self>,
        kind: &QString,
        start: f64,
        end: f64,
        props: &QMap<QMapPair_QString_QVariant>,
    ) -> QString {
        let kind = kind.to_string();
        let props = json_object(props);
        qstring(&self.edit(|document| document.add_overlay(&kind, start, end, None, &props)).unwrap_or_default())
    }

    pub(super) fn update_overlay(
        self: Pin<&mut Self>,
        id: &QString,
        patch: &QMap<QMapPair_QString_QVariant>,
        coalesce: bool,
    ) -> bool {
        let id = id.to_string();
        let patch = json_object(patch);
        self.edit(|document| document.update_overlay(&id, &patch, coalesce)).unwrap_or(false)
    }

    pub(super) fn remove_overlay(self: Pin<&mut Self>, id: &QString) -> bool {
        let id = id.to_string();
        self.edit(|document| document.remove_overlay(&id)).unwrap_or(false)
    }
}
