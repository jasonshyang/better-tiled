//! Image references as files state them.

use super::scalar::Scalar;

/// An `<image>` (in JSON, the `image`, `imagewidth`, `imageheight` and `transparentcolor` keys
/// of the element that has one).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawImage {
    /// The file, relative to the containing file.
    pub(crate) source: Option<Scalar>,
    /// The colour drawn as transparent: without `#` in XML, with it in JSON.
    pub(crate) trans: Option<Scalar>,
    /// The width the file was written for.
    pub(crate) width: Option<Scalar>,
    /// The height the file was written for.
    pub(crate) height: Option<Scalar>,
    /// The format of embedded bytes (`png`).
    pub(crate) format: Option<Scalar>,
    /// Embedded bytes.
    pub(crate) data: Option<RawImageData>,
}

/// An image's embedded `<data>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawImageData {
    /// The encoding (`base64`).
    pub(crate) encoding: Option<Scalar>,
    /// The encoded bytes.
    pub(crate) text: String,
}
