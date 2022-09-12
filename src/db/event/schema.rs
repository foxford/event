use csscolorparser::Color;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug)]
pub enum Event {
    Path(EventSchema),
    Other(EventSchema),
}

impl Event {
    pub fn compact(self) -> Result<CompactEvent, Error> {
        let evt = match self {
            Event::Path(evt) => CompactEvent::Path(CompactPathEvent::try_from_event(evt)?),
            Event::Other(evt) => CompactEvent::Other(evt),
        };

        Ok(evt)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum CompactEvent {
    Path(CompactPathEvent),
    Other(EventSchema),
}

impl CompactEvent {
    pub fn from_json(v: serde_json::Value) -> Result<Self, anyhow::Error> {
        let evt: Event = serde_json::from_value(v)?;
        let compacted = evt.compact()?;

        Ok(compacted)
    }

    pub fn into_json(self) -> Result<serde_json::Value, serde_json::Error> {
        let evt = match self {
            CompactEvent::Path(evt) => Event::Path(evt.into_event()),
            CompactEvent::Other(evt) => Event::Other(evt),
        };

        serde_json::to_value(evt)
    }
}

#[derive(Debug)]
pub enum Error {
    LosingPrecision,
    InvalidColor,
    MissingPath,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let disp = match self {
            Error::LosingPrecision => "compaction would loose precision",
            Error::InvalidColor => "event has invalid color",
            Error::MissingPath => "missing path for path event",
        };

        write!(f, "{:?}", disp)
    }
}

impl std::error::Error for Error {}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
enum GlobalCompositeOperation {
    #[default]
    SourceOver,
    SourceIn,
    SourceOut,
    SourceAtop,
    DestinationOver,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Lighter,
    Copy,
    Xor,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
enum Origin {
    Left,
    Right,
    Top,
    Center,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum PaintFirst {
    #[default]
    Fill,
    Stroke,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum StrokeLineCap {
    #[default]
    Round,
    Butt,
    Square,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum StrokeLineJoin {
    #[default]
    Round,
    Miter,
    Bevel,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
enum CrossOrigin {
    #[default]
    Anonymous,
    UseCredentials,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
    JustifyLeft,
    JustifyCenter,
    JustifyRight,
}

/// Do not support numbers (400, 600, 800).
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Path,
    Image,
    Rect,
    Circle,
    Textbox,
    #[serde(rename = "WhiteboardLine")]
    WhiteboardLine,
    Triangle,
}

/// This is the schema for each event type.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EventSchema {
    #[serde(rename = "_id")]
    _id: Uuid,

    origin_x: Origin,
    origin_y: Origin,
    top: f32,
    left: f32,
    height: f32,
    width: f32,

    angle: f32,
    flip_x: bool,
    flip_y: bool,
    skew_x: f32,
    skew_y: f32,
    // scale_x can be too large
    scale_x: f64,
    // scale_y too
    scale_y: f64,

    fill: Option<String>,
    fill_rule: FillRule,
    // Do not support shadows.
    // shadow: Option<serde_json::Value>,
    stroke: Option<Color>,
    opacity: u8,
    visible: bool,
    paint_first: PaintFirst,
    global_composite_operation: GlobalCompositeOperation,
    background_color: Option<String>,
    no_scale_cache: Option<bool>,
    stroke_dash_array: Option<Vec<f32>>,
    stroke_dash_offset: f32,
    stroke_line_cap: StrokeLineCap,
    stroke_line_join: StrokeLineJoin,
    stroke_miter_limit: u8,
    stroke_uniform: Option<bool>,
    stroke_width: u8,
    cross_origin: Option<CrossOrigin>,

    version: String,

    #[serde(rename = "type")]
    kind: Kind,

    // our custom fields
    #[serde(rename = "_removed")]
    _removed: Option<bool>,
    #[serde(rename = "_rev")]
    _rev: Option<Uuid>,
    #[serde(rename = "_restored")]
    _restored: Option<bool>,
    #[serde(rename = "_lockedbyuser")]
    _locked_by_user: Option<bool>,
    #[serde(rename = "_onlyState")]
    _only_state: Option<bool>,
    #[serde(rename = "_invalidate")]
    _invalidate: Option<bool>,

    // fields specific for some events

    // Path
    path: Option<Vec<serde_json::Value>>,

    // Image
    src: Option<String>,
    crop_x: Option<f32>,
    crop_y: Option<f32>,
    filters: Option<Vec<()>>,

    // Rect
    rx: Option<f32>,
    ry: Option<f32>,

    // Circle
    radius: Option<f32>,
    start_angle: Option<f32>,
    end_angle: Option<f32>,

    // Text
    text: Option<String>,
    // Do not support styles.
    // styles: Option<serde_json::Value>,
    font_size: Option<u8>,
    min_width: Option<f32>,
    overline: Option<bool>,
    // missing spec on values (only 'left' is known)
    path_side: Option<String>,
    direction: Option<Direction>,
    font_style: Option<FontStyle>,
    text_align: Option<TextAlign>,
    underline: Option<bool>,
    font_family: Option<String>,
    font_weight: Option<FontWeight>,
    line_height: Option<f32>,
    char_spacing: Option<f32>,
    linethrough: Option<bool>,
    path_start_offset: Option<f32>,
    split_by_grapheme: Option<bool>,
    text_background_color: Option<String>,

    // Whiteboard Line
    x1: Option<f32>,
    x2: Option<f32>,
    y1: Option<f32>,
    y2: Option<f32>,
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let evt = EventSchema::deserialize(deserializer)?;

        match evt.kind {
            Kind::Path => Ok(Event::Path(evt)),
            _ => Ok(Event::Other(evt)),
        }
    }
}

impl Serialize for Event {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Event::Path(evt) => evt.serialize(serializer),
            Event::Other(evt) => evt.serialize(serializer),
        }
    }
}

#[derive(Debug)]
pub enum PathPart {
    M(f64, f64),
    Q(f64, f64, f64, f64),
    L(f64, f64),
}

#[derive(Debug)]
pub struct Path {
    parts: Vec<PathPart>,
    min_x: f64,
    width: f64,
    min_y: f64,
    height: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CompactPathPart {
    M(u16, u16),
    Q(u16, u16, u16, u16),
    L(u16, u16),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompactPath {
    parts: Vec<CompactPathPart>,
    min_x: f32,
    width: f32,
    min_y: f32,
    height: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompactPathEvent {
    #[serde(rename = "_id")]
    _id: Uuid,

    origin_x: Origin,
    origin_y: Origin,
    top: i32,
    left: i32,
    height: u32,
    width: u32,

    angle: u16,
    flip_x: bool,
    flip_y: bool,
    skew_x: i16,
    skew_y: i16,
    scale_x: f64,
    scale_y: f64,

    fill: Option<Color>,
    fill_rule: FillRule,
    stroke: Option<Color>,
    opacity: u8,
    visible: bool,
    paint_first: PaintFirst,
    global_composite_operation: GlobalCompositeOperation,
    background_color: Option<Color>,
    no_scale_cache: bool,
    stroke_dash_array: Option<Vec<f32>>,
    stroke_dash_offset: u8,
    stroke_line_cap: StrokeLineCap,
    stroke_line_join: StrokeLineJoin,
    stroke_miter_limit: u8,
    stroke_uniform: Option<bool>,
    stroke_width: u8,

    #[allow(dead_code)]
    version: String,

    #[serde(rename = "type")]
    kind: Kind,

    // Path
    path: CompactPath,

    // our fields
    #[serde(rename = "_removed")]
    _removed: Option<bool>,
    #[serde(rename = "_rev")]
    _rev: Option<Uuid>,
    #[serde(rename = "_restored")]
    _restored: Option<bool>,
    #[serde(rename = "_lockedbyuser")]
    _locked_by_user: Option<bool>,
    #[serde(rename = "_onlyState")]
    _only_state: Option<bool>,
    #[serde(rename = "_invalidate")]
    _invalidate: Option<bool>,
}

fn decode_path(path: Vec<serde_json::Value>) -> Option<Path> {
    let mut decoded_path = Vec::with_capacity(path.len());

    fn as_f64(arr: &[serde_json::Value], idx: usize) -> Option<f64> {
        arr.get(idx)?.as_f64()
    }

    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    for part in path {
        let arr = part.as_array()?;
        let label = arr.get(0)?.as_str()?;

        let part = match label {
            "M" => {
                let x = as_f64(arr, 1)?;
                min_x = if x < min_x { x } else { min_x };
                max_x = if x > max_x { x } else { max_x };

                let y = as_f64(arr, 2)?;
                min_y = if y < min_y { y } else { min_y };
                max_y = if y > max_y { y } else { max_y };

                PathPart::M(x, y)
            }
            "L" => {
                let x = as_f64(arr, 1)?;
                min_x = if x < min_x { x } else { min_x };
                max_x = if x > max_x { x } else { max_x };

                let y = as_f64(arr, 2)?;
                min_y = if y < min_y { y } else { min_y };
                max_y = if y > max_y { y } else { max_y };

                PathPart::L(x, y)
            }
            "Q" => {
                let x1 = as_f64(arr, 1)?;
                min_x = if x1 < min_x { x1 } else { min_x };
                max_x = if x1 > max_x { x1 } else { max_x };
                let y1 = as_f64(arr, 2)?;
                min_y = if y1 < min_y { y1 } else { min_y };
                max_y = if y1 > max_y { y1 } else { max_y };

                let x2 = as_f64(arr, 3)?;
                min_x = if x2 < min_x { x2 } else { min_x };
                max_x = if x2 > max_x { x2 } else { max_x };
                let y2 = as_f64(arr, 4)?;
                min_y = if y2 < min_y { y2 } else { min_y };
                max_y = if y2 > max_y { y2 } else { max_y };

                PathPart::Q(x1, y1, x2, y2)
            }
            _ => return None,
        };

        decoded_path.push(part);
    }

    Some(Path {
        parts: decoded_path,
        min_x,
        min_y,
        height: max_y - min_y,
        width: max_x - min_x,
    })
}

fn compress_path(path: Vec<serde_json::Value>) -> Option<CompactPath> {
    let decoded = decode_path(path)?;
    let mut comp_path = Vec::with_capacity(decoded.parts.len());

    fn to_u16(x: f64) -> Option<u16> {
        match (x * 10000.0).trunc() as u64 {
            r if r < u16::MAX as u64 => Some(r as u16),
            _ => None,
        }
    }

    fn norm(x: f64, min: f64, length: f64) -> Option<u16> {
        let x = (x - min) / length;
        to_u16(x)
    }

    for part in decoded.parts {
        let part = match part {
            PathPart::M(x, y) => CompactPathPart::M(
                norm(x, decoded.min_x, decoded.width)?,
                norm(y, decoded.min_y, decoded.height)?,
            ),
            PathPart::Q(x1, y1, x2, y2) => CompactPathPart::Q(
                norm(x1, decoded.min_x, decoded.width)?,
                norm(y1, decoded.min_y, decoded.height)?,
                norm(x2, decoded.min_x, decoded.width)?,
                norm(y2, decoded.min_y, decoded.height)?,
            ),
            PathPart::L(x, y) => CompactPathPart::L(
                norm(x, decoded.min_x, decoded.width)?,
                norm(y, decoded.min_y, decoded.height)?,
            ),
        };

        comp_path.push(part);
    }

    Some(CompactPath {
        parts: comp_path,
        min_x: decoded.min_x as f32,
        width: decoded.width as f32,
        min_y: decoded.min_y as f32,
        height: decoded.height as f32,
    })
}

fn decompress_path(path: CompactPath) -> Path {
    let mut parts = Vec::with_capacity(path.parts.len());

    fn denorm(x: u16, min: f64, length: f64) -> f64 {
        min + ((x as f64) / 10000.0) * length
    }

    let min_x = path.min_x as f64;
    let min_y = path.min_y as f64;
    let width = path.width as f64;
    let height = path.height as f64;

    for part in path.parts {
        let decompressed_part = match part {
            CompactPathPart::M(x, y) => {
                PathPart::M(denorm(x, min_x, width), denorm(y, min_y, height))
            }
            CompactPathPart::Q(x1, y1, x2, y2) => PathPart::Q(
                denorm(x1, min_x, width),
                denorm(y1, min_y, height),
                denorm(x2, min_x, width),
                denorm(y2, min_y, height),
            ),
            CompactPathPart::L(x, y) => {
                PathPart::L(denorm(x, min_x, width), denorm(y, min_y, height))
            }
        };

        parts.push(decompressed_part);
    }

    Path {
        parts,
        min_x,
        width,
        min_y,
        height,
    }
}

fn two_decimal_places(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn path_to_json(path: Path) -> Vec<serde_json::Value> {
    let mut value = vec![];

    for part in path.parts {
        match part {
            PathPart::M(x, y) => value.push(serde_json::json!([
                "M",
                two_decimal_places(x),
                two_decimal_places(y)
            ])),
            PathPart::Q(x1, y1, x2, y2) => value.push(serde_json::json!([
                "Q",
                two_decimal_places(x1),
                two_decimal_places(y1),
                two_decimal_places(x2),
                two_decimal_places(y2)
            ])),
            PathPart::L(x, y) => value.push(serde_json::json!([
                "L",
                two_decimal_places(x),
                two_decimal_places(y)
            ])),
        }
    }

    value
}

fn f32tu32(f: f32) -> Option<u32> {
    match (f * 100.0).trunc() {
        f if (f as u64) < u32::MAX as u64 => Some(f as u32),
        _ => None,
    }
}

fn f32tu16(f: f32) -> Option<u16> {
    match (f * 100.0).trunc() {
        f if (f as u64) < u16::MAX as u64 => Some(f as u16),
        _ => None,
    }
}

fn f32tu8(f: f32) -> Option<u8> {
    match (f * 100.0).trunc() {
        f if (f as u64) < u8::MAX as u64 => Some(f as u8),
        _ => None,
    }
}

fn f32ti32(f: f32) -> Option<i32> {
    match (f * 100.0).trunc() as i64 {
        f if f < i32::MAX as i64 && f > i32::MIN as i64 => Some(f as i32),
        _ => None,
    }
}

fn f32ti16(f: f32) -> Option<i16> {
    match (f * 100.0).trunc() as i64 {
        f if f < i32::MAX as i64 && f > i32::MIN as i64 => Some(f as i16),
        _ => None,
    }
}

impl CompactPathEvent {
    fn try_from_event(e: EventSchema) -> Result<Self, Error> {
        fn opt_color(c: Option<String>) -> Result<Option<Color>, Error> {
            match c {
                Some(s) if s.is_empty() => Ok(None),
                Some(s) => Ok(Some(s.parse().map_err(|_err| Error::InvalidColor)?)),
                None => Ok(None),
            }
        }

        let fill = opt_color(e.fill)?;
        let background_color = opt_color(e.background_color)?;

        Ok(Self {
            _id: e._id,
            origin_x: e.origin_x,
            origin_y: e.origin_y,
            top: f32ti32(e.top).ok_or(Error::LosingPrecision)?,
            left: f32ti32(e.left).ok_or(Error::LosingPrecision)?,
            height: f32tu32(e.height).ok_or(Error::LosingPrecision)?,
            width: f32tu32(e.width).ok_or(Error::LosingPrecision)?,
            angle: f32tu16(e.angle).ok_or(Error::LosingPrecision)?,
            flip_x: e.flip_x,
            flip_y: e.flip_y,
            skew_x: f32ti16(e.skew_x).ok_or(Error::LosingPrecision)?,
            skew_y: f32ti16(e.skew_y).ok_or(Error::LosingPrecision)?,
            scale_x: e.scale_x,
            scale_y: e.scale_y,
            fill,
            fill_rule: e.fill_rule,
            stroke: e.stroke,
            opacity: e.opacity as u8,
            visible: e.visible,
            paint_first: e.paint_first,
            global_composite_operation: e.global_composite_operation,
            background_color,
            no_scale_cache: e.no_scale_cache.unwrap_or(true),
            stroke_dash_array: e.stroke_dash_array,
            stroke_dash_offset: f32tu8(e.stroke_dash_offset).ok_or(Error::LosingPrecision)?,
            stroke_line_cap: e.stroke_line_cap,
            stroke_line_join: e.stroke_line_join,
            stroke_miter_limit: e.stroke_miter_limit,
            stroke_uniform: e.stroke_uniform,
            stroke_width: e.stroke_width,
            version: e.version,
            kind: e.kind,
            path: compress_path(e.path.ok_or(Error::MissingPath)?).ok_or(Error::LosingPrecision)?,
            _removed: e._removed,
            _rev: e._rev,
            _restored: e._restored,
            _locked_by_user: e._locked_by_user,
            _only_state: e._only_state,
            _invalidate: e._invalidate,
        })
    }

    fn into_event(self) -> EventSchema {
        EventSchema {
            _id: self._id,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            top: two_decimal_places(self.top as f64 / 100.0) as f32,
            left: two_decimal_places(self.left as f64 / 100.0) as f32,
            height: two_decimal_places(self.height as f64 / 100.0) as f32,
            width: two_decimal_places(self.width as f64 / 100.0) as f32,
            angle: two_decimal_places(self.angle as f64 / 100.0) as f32,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
            skew_x: two_decimal_places(self.skew_x as f64 / 100.0) as f32,
            skew_y: two_decimal_places(self.skew_y as f64 / 100.0) as f32,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            fill: self.fill.map(|f| f.to_rgb_string()),
            fill_rule: self.fill_rule,
            stroke: self.stroke,
            opacity: self.opacity,
            visible: self.visible,
            paint_first: self.paint_first,
            global_composite_operation: self.global_composite_operation,
            background_color: self.background_color.map(|c| c.to_rgb_string()),
            no_scale_cache: Some(self.no_scale_cache),
            stroke_dash_array: self.stroke_dash_array,
            stroke_dash_offset: self.stroke_dash_offset as f32,
            stroke_line_cap: self.stroke_line_cap,
            stroke_line_join: self.stroke_line_join,
            stroke_miter_limit: self.stroke_miter_limit,
            stroke_uniform: self.stroke_uniform,
            stroke_width: self.stroke_width,
            version: self.version,
            kind: self.kind,
            path: Some(path_to_json(decompress_path(self.path))),

            _removed: self._removed,
            _rev: self._rev,
            _restored: self._restored,
            _locked_by_user: self._locked_by_user,
            _only_state: self._only_state,
            _invalidate: self._invalidate,

            cross_origin: None,
            src: None,
            crop_x: None,
            crop_y: None,
            filters: None,
            rx: None,
            ry: None,
            radius: None,
            start_angle: None,
            end_angle: None,
            text: None,
            font_size: None,
            min_width: None,
            overline: None,
            path_side: None,
            direction: None,
            font_style: None,
            text_align: None,
            underline: None,
            font_family: None,
            font_weight: None,
            line_height: None,
            char_spacing: None,
            linethrough: None,
            path_start_offset: None,
            split_by_grapheme: None,
            text_background_color: None,
            x1: None,
            x2: None,
            y1: None,
            y2: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_weight() {
        #[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
        struct Test {
            #[serde(skip_serializing_if = "Option::is_none")]
            t: Option<FontWeight>,
        }

        let de_cases = [
            (
                "{\"t\": \"bold\"}",
                Test {
                    t: Some(FontWeight::Bold),
                },
            ),
            (
                "{\"t\": \"normal\"}",
                Test {
                    t: Some(FontWeight::Normal),
                },
            ),
            ("{}", Test { t: None }),
        ];

        for (input, should_be) in de_cases {
            assert_eq!(
                serde_json::from_str::<Test>(input).expect(input),
                should_be,
                "failed on input {}",
                input
            );
        }

        let ser_cases = [
            (
                Test {
                    t: Some(FontWeight::Bold),
                },
                "{\"t\":\"bold\"}",
            ),
            (
                Test {
                    t: Some(FontWeight::Normal),
                },
                "{\"t\":\"normal\"}",
            ),
            (Test { t: None }, "{}"),
        ];

        for (input, should_be) in ser_cases {
            assert_eq!(
                serde_json::to_string(&input).expect(should_be),
                should_be,
                "failed on {}",
                should_be
            );
        }
    }
}
