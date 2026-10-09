//! What finishing one file needs: the loader for the files it refers to, the folder references
//! resolve against, and the warnings found so far; with the lenient conversions of stated values.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Color, CustomTypes};

use crate::loader::Loader;
use crate::paths::file_ref;
use crate::raw::Scalar;
use crate::warning::{Warning, WarningKind, Warnings};

/// Finishes the raw tree of one file.
pub(crate) struct Finisher<'l> {
    /// The loader, for the tilesets and templates the file refers to.
    pub(crate) loader: &'l mut Loader,
    /// The folder of the file.
    pub(crate) dir: PathBuf,
    /// The problems found.
    pub(crate) warnings: Warnings,
    /// The loader's custom types.
    pub(crate) types: Arc<CustomTypes>,
}

impl<'l> Finisher<'l> {
    /// Finishes the file at `file` (an absolute path).
    pub(crate) fn new(loader: &'l mut Loader, file: &Path) -> Finisher<'l> {
        Finisher {
            types: Arc::clone(loader.custom_types()),
            loader,
            dir: file.parent().map(Path::to_path_buf).unwrap_or_default(),
            warnings: Warnings::new(file),
        }
    }

    /// The warnings found.
    pub(crate) fn into_warnings(self) -> Vec<Warning> {
        self.warnings.into_list()
    }

    /// A reference written in this file.
    pub(crate) fn file_ref(&self, reference: &str) -> bt_core::FileRef {
        file_ref(&self.dir, reference)
    }

    /// Records that `what` was stated as something that is not `expected`.
    fn record_invalid(&mut self, what: fmt::Arguments<'_>, value: &Scalar, expected: &str) {
        self.warnings.record(
            WarningKind::InvalidValue,
            format!(
                "{what} is “{}”, which is not {expected}; the default is used",
                value.to_text()
            ),
        );
    }

    /// A stated value converted by `convert`, recording a warning when it does not convert.
    /// Empty text counts as not stated.
    fn converted<T>(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
        expected: &str,
        convert: impl FnOnce(&Scalar) -> Option<T>,
    ) -> Option<T> {
        let value = value.as_ref().filter(|v| !v.is_empty_text())?;
        let converted = convert(value);
        if converted.is_none() {
            self.record_invalid(what, value, expected);
        }
        converted
    }

    /// A 32-bit integer.
    pub(crate) fn i32_of(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
    ) -> Option<i32> {
        self.converted(value, what, "a whole number", |v| {
            v.to_int().and_then(|i| i32::try_from(i).ok())
        })
    }

    /// A non-negative 32-bit integer.
    pub(crate) fn u32_of(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
    ) -> Option<u32> {
        self.converted(value, what, "a whole number of zero or more", |v| {
            v.to_int().and_then(|i| u32::try_from(i).ok())
        })
    }

    /// A real.
    pub(crate) fn real_of(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
    ) -> Option<f64> {
        self.converted(value, what, "a number", |v| {
            v.to_real().filter(|r| r.is_finite())
        })
    }

    /// A boolean.
    pub(crate) fn bool_of(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
    ) -> Option<bool> {
        self.converted(value, what, "true or false", Scalar::to_bool)
    }

    /// A colour (`#rrggbb` or `#aarrggbb`, with or without `#`).
    pub(crate) fn color_of(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
    ) -> Option<Color> {
        self.converted(value, what, "a colour", |v| {
            Color::from_tiled_text(&v.to_text())
        })
    }

    /// An enumerated value by its name.
    pub(crate) fn named_of<T>(
        &mut self,
        value: &Option<Scalar>,
        what: fmt::Arguments<'_>,
        from_name: impl FnOnce(&str) -> Option<T>,
    ) -> Option<T> {
        self.converted(value, what, "one of its names", |v| {
            from_name(v.to_text().trim())
        })
    }

    /// Records a problem.
    pub(crate) fn record(&mut self, kind: WarningKind, message: impl Into<String>) {
        self.warnings.record(kind, message);
    }
}

/// Stated text; empty when not stated.
pub(crate) fn text_of(value: &Option<Scalar>) -> String {
    value.as_ref().map(Scalar::to_text).unwrap_or_default()
}
