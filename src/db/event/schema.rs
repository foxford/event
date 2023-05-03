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
            Event::Other(evt) => CompactEvent::Other(CompactEventSchema::from_event(evt)),
        };

        Ok(evt)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum CompactEvent {
    Path(CompactPathEvent),
    Other(CompactEventSchema),
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
            CompactEvent::Other(evt) => Event::Other(evt.into_event()),
        };

        serde_json::to_value(evt)
    }

    #[cfg(test)]
    pub fn test_rect_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Rect,
            x1: Some(1.0),
            x2: Some(10.0),
            y1: Some(1.0),
            y2: Some(10.0),
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }

    #[cfg(test)]
    pub fn test_line_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::WhiteboardLine,
            x1: Some(1.0),
            x2: Some(10.0),
            y1: Some(1.0),
            y2: Some(10.0),
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }

    #[cfg(test)]
    pub fn test_circle_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Circle,
            radius: Some(10.0),
            start_angle: Some(0.0),
            end_angle: Some(6.283185307179586),
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }

    #[cfg(test)]
    pub fn test_image_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Image,
            src: Some("storage://00000000-0000-0000-0000-000000000000.png".to_owned()),
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }

    #[cfg(test)]
    pub fn test_path_event() -> Self {
        let evt = EventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Path,
            path: Some(vec![
                serde_json::json!(["M", -6.54, 5614.2]),
                serde_json::json!(["Q", -6.54, 5614.19, -6.54, 5613.08]),
                serde_json::json!(["Q", -6.54, 5611.98, -6.54, 5610.87]),
                serde_json::json!(["Q", -6.54, 5609.76, -6.54, 5608.65]),
                serde_json::json!(["Q", -6.54, 5607.54, -5.43, 5605.87]),
                serde_json::json!(["Q", -4.32, 5604.21, -3.77, 5603.1]),
                serde_json::json!(["Q", -3.21, 5601.99, -2.1, 5600.32]),
                serde_json::json!(["Q", -0.99, 5598.66, -0.43, 5597.55]),
                serde_json::json!(["Q", 0.12, 5596.44, 1.23, 5595.33]),
                serde_json::json!(["Q", 2.34, 5594.22, 3.45, 5593.11]),
                serde_json::json!(["Q", 4.57, 5592, 6.23, 5590.89]),
                serde_json::json!(["Q", 7.9, 5589.78, 9.01, 5589.23]),
                serde_json::json!(["Q", 10.12, 5588.67, 11.23, 5588.12]),
                serde_json::json!(["Q", 12.34, 5587.56, 13.45, 5587.56]),
                serde_json::json!(["Q", 14.56, 5587.56, 16.23, 5586.45]),
                serde_json::json!(["L", 17.9, 5585.34]),
            ]),
            ..EventSchema::placeholder()
        };

        Self::Path(CompactPathEvent::try_from_event(evt).unwrap())
    }

    #[cfg(test)]
    pub fn test_textbox_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Textbox,
            text: Some("text".to_owned()),
            font_size: Some(40),
            min_width: Some(20.0),
            overline: Some(false),
            path_side: Some("left".to_owned()),
            direction: Some(Direction::Ltr),
            font_style: Some(FontStyle::Normal),
            text_align: Some(TextAlign::Left),
            underline: Some(false),
            font_family: Some("-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Roboto\", \"Oxygen\", \"Ubuntu\", \"Cantarell\", \"Fira Sans\", \"Droid Sans\", \"Helvetica Neue\", sans-serif".to_owned()),
            font_weight: Some(serde_json::json!("normal").to_string()),
            line_height: Some(1.16),
            char_spacing: Some(0.0),
            linethrough: Some(true),
            path_start_offset: Some(0.0),
            split_by_grapheme: Some(false),
            text_background_color: Some("".to_owned()),
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }

    #[cfg(test)]
    pub fn test_triangle_event() -> Self {
        Self::Other(CompactEventSchema {
            top: 1.0,
            left: 1.0,
            height: 10.0,
            width: 10.0,
            kind: Kind::Triangle,
            ..CompactEventSchema::from_event(EventSchema::placeholder())
        })
    }
}

#[derive(Debug)]
pub enum Error {
    LosingPrecision,
    MissingPath,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let disp = match self {
            Error::LosingPrecision => "compaction would loose precision",
            Error::MissingPath => "missing path for path event",
        };

        write!(f, "{disp:?}")
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
    Bottom,
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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
    #[serde(rename = "WhiteboardArrowLine")]
    WhiteboardArrowLine,
    #[serde(rename = "WhiteboardCircle")]
    WhiteboardCircle,
    #[serde(rename = "activeSelection")]
    ActiveSelection,
    Line,
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
    #[serde(with = "json_as_str", default)]
    shadow: Option<String>,
    stroke: Option<String>,
    opacity: u8,
    visible: bool,
    paint_first: PaintFirst,
    global_composite_operation: GlobalCompositeOperation,
    background_color: Option<String>,
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
    no_scale_cache: Option<bool>,
    #[serde(rename = "_removed", skip_serializing_if = "Option::is_none")]
    _removed: Option<bool>,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    _rev: Option<Uuid>,
    #[serde(rename = "_restored", skip_serializing_if = "Option::is_none")]
    _restored: Option<bool>,
    #[serde(rename = "_lockedbyuser", skip_serializing_if = "Option::is_none")]
    _locked_by_user: Option<bool>,
    #[serde(rename = "_onlyState", skip_serializing_if = "Option::is_none")]
    _only_state: Option<bool>,
    #[serde(rename = "_invalidate", skip_serializing_if = "Option::is_none")]
    _invalidate: Option<bool>,
    #[serde(rename = "_order", skip_serializing_if = "Option::is_none")]
    _order: Option<i64>,
    #[serde(rename = "_noHistory", skip_serializing_if = "Option::is_none")]
    _no_history: Option<bool>,
    #[serde(rename = "_drawByStretch", skip_serializing_if = "Option::is_none")]
    _draw_by_stretch: Option<bool>,

    // fields specific for some events

    // Path
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Vec<serde_json::Value>>,

    // Image
    #[serde(skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crop_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crop_y: Option<f32>,
    #[serde(with = "json_as_str", default)]
    filters: Option<String>,

    // Rect
    #[serde(skip_serializing_if = "Option::is_none")]
    rx: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ry: Option<f32>,

    // Circle
    #[serde(skip_serializing_if = "Option::is_none")]
    radius: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_angle: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_angle: Option<f32>,

    // Text
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(with = "json_as_str", default)]
    styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_size: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overline: Option<bool>,
    // missing spec on values (only 'left' is known)
    #[serde(skip_serializing_if = "Option::is_none")]
    path_side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<Direction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_style: Option<FontStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_align: Option<TextAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    font_family: Option<String>,
    #[serde(with = "json_as_str", default)]
    font_weight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line_height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    char_spacing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linethrough: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path_start_offset: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    split_by_grapheme: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_background_color: Option<String>,

    // Whiteboard Line
    #[serde(skip_serializing_if = "Option::is_none")]
    x1: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    x2: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    y1: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    y2: Option<f32>,
}

impl EventSchema {
    // Intended for event generation. Generates incorrect
    // event with sane default values.
    #[cfg(test)]
    fn placeholder() -> Self {
        Self {
            _id: Uuid::new_v4(),
            origin_x: Origin::Left,
            origin_y: Origin::Top,
            top: Default::default(),
            left: Default::default(),
            height: Default::default(),
            width: Default::default(),
            angle: Default::default(),
            flip_x: Default::default(),
            flip_y: Default::default(),
            skew_x: Default::default(),
            skew_y: Default::default(),
            scale_x: Default::default(),
            scale_y: Default::default(),
            fill: Default::default(),
            fill_rule: Default::default(),
            stroke: Default::default(),
            opacity: 1,
            visible: true,
            paint_first: Default::default(),
            global_composite_operation: Default::default(),
            background_color: Default::default(),
            no_scale_cache: Some(true),
            shadow: None,
            filters: None,
            stroke_dash_array: Default::default(),
            stroke_dash_offset: Default::default(),
            stroke_line_cap: Default::default(),
            stroke_line_join: Default::default(),
            stroke_miter_limit: 10,
            stroke_uniform: Default::default(),
            stroke_width: Default::default(),
            cross_origin: Default::default(),
            version: "4.6.0".to_owned(),
            kind: Kind::WhiteboardLine,
            _removed: Default::default(),
            _rev: Default::default(),
            _restored: Default::default(),
            _locked_by_user: Default::default(),
            _only_state: Default::default(),
            _invalidate: Default::default(),
            _order: Option::Some(0),
            _no_history: Default::default(),
            _draw_by_stretch: Default::default(),
            path: Default::default(),
            src: Default::default(),
            crop_x: Default::default(),
            crop_y: Default::default(),
            rx: Default::default(),
            ry: Default::default(),
            radius: Default::default(),
            start_angle: Default::default(),
            end_angle: Default::default(),
            text: Default::default(),
            font_size: Default::default(),
            min_width: Default::default(),
            overline: Default::default(),
            path_side: Default::default(),
            direction: Default::default(),
            font_style: Default::default(),
            text_align: Default::default(),
            underline: Default::default(),
            font_family: Default::default(),
            font_weight: Default::default(),
            line_height: Default::default(),
            char_spacing: Default::default(),
            linethrough: Default::default(),
            path_start_offset: Default::default(),
            split_by_grapheme: Default::default(),
            text_background_color: Default::default(),
            x1: Default::default(),
            x2: Default::default(),
            y1: Default::default(),
            y2: Default::default(),
            styles: None,
        }
    }
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

mod json_as_str {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn deserialize<'de, D>(d: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(d)?;
        Ok(value.map(|v| v.to_string()))
    }

    pub fn serialize<S>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value: Option<serde_json::Value> = match v {
            Some(v) => {
                let v = serde_json::from_str(v).map_err(serde::ser::Error::custom)?;
                Some(v)
            }
            None => None,
        };
        value.serialize(s)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompactEventSchema {
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
    shadow: Option<String>,
    stroke: Option<String>,
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
    _removed: Option<bool>,
    _rev: Option<Uuid>,
    _restored: Option<bool>,
    _locked_by_user: Option<bool>,
    _only_state: Option<bool>,
    _invalidate: Option<bool>,
    _order: Option<i64>,
    _no_history: Option<bool>,
    _draw_by_stretch: Option<bool>,

    // fields specific for some events

    // Path
    path: Option<Vec<String>>,

    // Image
    src: Option<String>,
    crop_x: Option<f32>,
    crop_y: Option<f32>,
    filters: Option<String>,

    // Rect
    rx: Option<f32>,
    ry: Option<f32>,

    // Circle
    radius: Option<f32>,
    start_angle: Option<f32>,
    end_angle: Option<f32>,

    // Text
    text: Option<String>,
    styles: Option<String>,
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
    font_weight: Option<String>,
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

impl CompactEventSchema {
    fn from_event(e: EventSchema) -> Self {
        Self {
            _id: e._id,
            origin_x: e.origin_x,
            origin_y: e.origin_y,
            top: e.top,
            left: e.left,
            height: e.height,
            width: e.width,
            angle: e.angle,
            flip_x: e.flip_x,
            flip_y: e.flip_y,
            skew_x: e.skew_x,
            skew_y: e.skew_y,
            scale_x: e.scale_x,
            scale_y: e.scale_y,
            fill: e.fill,
            fill_rule: e.fill_rule,
            shadow: e.shadow,
            stroke: e.stroke,
            opacity: e.opacity,
            visible: e.visible,
            paint_first: e.paint_first,
            global_composite_operation: e.global_composite_operation,
            background_color: e.background_color,
            no_scale_cache: e.no_scale_cache,
            stroke_dash_array: e.stroke_dash_array,
            stroke_dash_offset: e.stroke_dash_offset,
            stroke_line_cap: e.stroke_line_cap,
            stroke_line_join: e.stroke_line_join,
            stroke_miter_limit: e.stroke_miter_limit,
            stroke_uniform: e.stroke_uniform,
            stroke_width: e.stroke_width,
            cross_origin: e.cross_origin,
            version: e.version,
            kind: e.kind,
            _removed: e._removed,
            _rev: e._rev,
            _restored: e._restored,
            _locked_by_user: e._locked_by_user,
            _only_state: e._only_state,
            _invalidate: e._invalidate,
            _order: e._order,
            _no_history: e._no_history,
            _draw_by_stretch: e._draw_by_stretch,
            path: e
                .path
                .map(|p| p.into_iter().map(|p| p.to_string()).collect::<Vec<_>>()),
            src: e.src,
            crop_x: e.crop_x,
            crop_y: e.crop_y,
            filters: e.filters,
            rx: e.rx,
            ry: e.ry,
            radius: e.radius,
            start_angle: e.start_angle,
            end_angle: e.end_angle,
            text: e.text,
            styles: e.styles,
            font_size: e.font_size,
            min_width: e.min_width,
            overline: e.overline,
            path_side: e.path_side,
            direction: e.direction,
            font_style: e.font_style,
            text_align: e.text_align,
            underline: e.underline,
            font_family: e.font_family,
            font_weight: e.font_weight,
            line_height: e.line_height,
            char_spacing: e.char_spacing,
            linethrough: e.linethrough,
            path_start_offset: e.path_start_offset,
            split_by_grapheme: e.split_by_grapheme,
            text_background_color: e.text_background_color,
            x1: e.x1,
            x2: e.x2,
            y1: e.y1,
            y2: e.y2,
        }
    }

    fn into_event(self) -> EventSchema {
        EventSchema {
            _id: self._id,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            top: self.top,
            left: self.left,
            height: self.height,
            width: self.width,
            angle: self.angle,
            flip_x: self.flip_x,
            flip_y: self.flip_y,
            skew_x: self.skew_x,
            skew_y: self.skew_y,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            fill: self.fill,
            fill_rule: self.fill_rule,
            shadow: self.shadow,
            stroke: self.stroke,
            opacity: self.opacity,
            visible: self.visible,
            paint_first: self.paint_first,
            global_composite_operation: self.global_composite_operation,
            background_color: self.background_color,
            no_scale_cache: self.no_scale_cache,
            stroke_dash_array: self.stroke_dash_array,
            stroke_dash_offset: self.stroke_dash_offset,
            stroke_line_cap: self.stroke_line_cap,
            stroke_line_join: self.stroke_line_join,
            stroke_miter_limit: self.stroke_miter_limit,
            stroke_uniform: self.stroke_uniform,
            stroke_width: self.stroke_width,
            cross_origin: self.cross_origin,
            version: self.version,
            kind: self.kind,
            _removed: self._removed,
            _rev: self._rev,
            _restored: self._restored,
            _locked_by_user: self._locked_by_user,
            _only_state: self._only_state,
            _invalidate: self._invalidate,
            _order: self._order,
            _no_history: self._no_history,
            _draw_by_stretch: self._draw_by_stretch,
            path: self.path.map(|p| {
                p.into_iter()
                    .flat_map(|p| serde_json::from_str(&p))
                    .collect::<Vec<_>>()
            }),
            src: self.src,
            crop_x: self.crop_x,
            crop_y: self.crop_y,
            filters: self.filters,
            rx: self.rx,
            ry: self.ry,
            radius: self.radius,
            start_angle: self.start_angle,
            end_angle: self.end_angle,
            text: self.text,
            styles: self.styles,
            font_size: self.font_size,
            min_width: self.min_width,
            overline: self.overline,
            path_side: self.path_side,
            direction: self.direction,
            font_style: self.font_style,
            text_align: self.text_align,
            underline: self.underline,
            font_family: self.font_family,
            font_weight: self.font_weight,
            line_height: self.line_height,
            char_spacing: self.char_spacing,
            linethrough: self.linethrough,
            path_start_offset: self.path_start_offset,
            split_by_grapheme: self.split_by_grapheme,
            text_background_color: self.text_background_color,
            x1: self.x1,
            x2: self.x2,
            y1: self.y1,
            y2: self.y2,
        }
    }
}

#[derive(Debug)]
pub enum PathPart {
    M(f64, f64),
    Q(f64, f64, f64, f64),
    L(f64, f64),
    Z,
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
    Z,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompactPath {
    parts: Vec<CompactPathPart>,
    min_x: f32,
    width: f32,
    min_y: f32,
    height: f32,
}

impl CompactPath {
    #[cfg(test)]
    fn len(&self) -> usize {
        self.parts.len()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompactPathEvent {
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

    fill: Option<String>,
    fill_rule: FillRule,
    shadow: Option<String>,
    stroke: Option<String>,
    opacity: u8,
    visible: bool,
    paint_first: PaintFirst,
    global_composite_operation: GlobalCompositeOperation,
    background_color: Option<String>,
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
    _removed: Option<bool>,
    _rev: Option<Uuid>,
    _restored: Option<bool>,
    _locked_by_user: Option<bool>,
    _only_state: Option<bool>,
    _invalidate: Option<bool>,
    _order: Option<i64>,
    _no_history: Option<bool>,
    _draw_by_stretch: Option<bool>,
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
            "z" => PathPart::Z,
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
            PathPart::Z => CompactPathPart::Z,
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
            CompactPathPart::Z => PathPart::Z,
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
            PathPart::Z => value.push(serde_json::json!(["z"])),
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
            fill: e.fill,
            fill_rule: e.fill_rule,
            shadow: e.shadow,
            stroke: e.stroke,
            opacity: e.opacity,
            visible: e.visible,
            paint_first: e.paint_first,
            global_composite_operation: e.global_composite_operation,
            background_color: e.background_color,
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
            _order: e._order,
            _no_history: e._no_history,
            _draw_by_stretch: e._draw_by_stretch,
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
            fill: self.fill,
            fill_rule: self.fill_rule,
            shadow: self.shadow,
            stroke: self.stroke,
            opacity: self.opacity,
            visible: self.visible,
            paint_first: self.paint_first,
            global_composite_operation: self.global_composite_operation,
            background_color: self.background_color,
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
            _order: self._order,
            _no_history: self._no_history,
            _draw_by_stretch: self._draw_by_stretch,

            cross_origin: None,
            src: None,
            crop_x: None,
            crop_y: None,
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
            filters: None,
            styles: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let evts = [
            CompactEvent::test_rect_event(),
            CompactEvent::test_line_event(),
            CompactEvent::test_circle_event(),
            CompactEvent::test_image_event(),
            CompactEvent::test_path_event(),
            CompactEvent::test_triangle_event(),
            CompactEvent::test_textbox_event(),
        ];

        for evt in evts {
            let json_value = evt.into_json().unwrap();
            let evt = CompactEvent::from_json(json_value).unwrap();

            let postcard_binary = postcard::to_allocvec(&evt).unwrap();
            let evt: CompactEvent = postcard::from_bytes(&postcard_binary).unwrap();

            evt.into_json().unwrap();
        }

        let raw_evts = [
            r#"{"type":"path","version":"4.6.0","originX":"left","originY":"bottom","left":692.24,"top":307.87,"width":97.2,"height":97.2,"fill":"rgba(0,0,0,0.009)","stroke":"rgba(0,0,0,1)","strokeWidth":2,"strokeDashArray":null,"strokeLineCap":"butt","strokeDashOffset":0,"strokeLineJoin":"miter","strokeUniform":true,"strokeMiterLimit":40,"scaleX":1.81,"scaleY":0.98,"angle":0,"flipX":false,"flipY":true,"opacity":1,"shadow":null,"visible":true,"backgroundColor":"","fillRule":"nonzero","paintFirst":"fill","globalCompositeOperation":"source-over","skewX":0,"skewY":0,"_id":"8a565154-8155-433a-a77c-81dc7168394c","noScaleCache":false,"_order":11,"_drawByStretch":true,"path":[["M",0,0],["L",97.2,0],["L",0,97.2],["z"]]}"#,
            r#"{"rx": 0, "ry": 0, "_id": "c900cf81-8af6-4eb2-82fe-4be2b2eab5b9", "top": 265.76, "fill": "rgba(255,255,255,1)", "left": 440.13, "type": "rect", "angle": 0, "flipX": false, "flipY": false, "skewX": 0, "skewY": 0, "width": 398.96, "_order": -1, "height": 133.43, "scaleX": 1, "scaleY": 1, "shadow": null, "stroke": "rgba(255,255,255,1)", "opacity": 1, "originX": "left", "originY": "top", "version": "4.6.0", "visible": true, "fillRule": "nonzero", "_noHistory": null, "paintFirst": "fill", "strokeWidth": 2, "noScaleCache": false, "_lockedbyuser": null, "strokeLineCap": "butt", "strokeUniform": true, "_drawByStretch": true, "strokeLineJoin": "miter", "backgroundColor": "", "strokeDashArray": null, "strokeDashOffset": 0, "strokeMiterLimit": 4, "globalCompositeOperation": "source-over"}"#,
        ];

        for raw in raw_evts {
            let evt = serde_json::from_str(raw).unwrap();
            let evt = CompactEvent::from_json(evt).unwrap();

            let postcard_binary = postcard::to_allocvec(&evt).unwrap();
            let evt: CompactEvent = postcard::from_bytes(&postcard_binary).unwrap();

            evt.into_json().unwrap();
        }

        let hex_evts = [
            "0110c900cf818af64eb282fe4be2b2eab5b90002cd4c85438f02e243146e0543e17ac7430000000000000000000000000000000000000000f03f000000000000f03f011372676261283235352c3235352c3235352c31290000011372676261283235352c3235352c3235352c3129010100000100010000000000000101040101020005342e362e3002000000000000010000010100000000000100000000010000000000000000000000000000000000000000000000000000000000",
            "0110c900cf818af64eb282fe4be2b2eab5b90002cd4c85438f02e243146e0543e17ac7430000000000000000000000000000000000000000f03f000000000000f03f011372676261283235352c3235352c3235352c31290000011372676261283235352c3235352c3235352c3129010100000100010000000000000101040101020005342e362e3002000000000000010100010100000000000100000000010000000000000000000000000000000000000000000000000000000000"
        ];
        for raw in hex_evts {
            let evt = hex::decode(raw).unwrap();
            let evt: CompactEvent = postcard::from_bytes(&evt).unwrap();

            evt.into_json().unwrap();
        }
    }

    #[test]
    fn test_path_encode_decode() {
        let original = vec![
            serde_json::json!(["M", -6.54, 5614.2]),
            serde_json::json!(["Q", -6.54, 5614.19, -6.54, 5613.08]),
            serde_json::json!(["Q", -6.54, 5611.98, -6.54, 5610.87]),
            serde_json::json!(["Q", -6.54, 5609.76, -6.54, 5608.65]),
            serde_json::json!(["Q", -6.54, 5607.54, -5.43, 5605.87]),
            serde_json::json!(["Q", -4.32, 5604.21, -3.77, 5603.1]),
            serde_json::json!(["Q", -3.21, 5601.99, -2.1, 5600.32]),
            serde_json::json!(["Q", -0.99, 5598.66, -0.43, 5597.55]),
            serde_json::json!(["Q", 0.12, 5596.44, 1.23, 5595.33]),
            serde_json::json!(["Q", 2.34, 5594.22, 3.45, 5593.11]),
            serde_json::json!(["Q", 4.57, 5592.0, 6.23, 5590.89]),
            serde_json::json!(["Q", 7.9, 5589.78, 9.01, 5589.23]),
            serde_json::json!(["Q", 10.12, 5588.67, 11.23, 5588.12]),
            serde_json::json!(["Q", 12.34, 5587.56, 13.45, 5587.56]),
            serde_json::json!(["Q", 14.56, 5587.56, 16.23, 5586.45]),
            serde_json::json!(["L", 17.9, 5585.34]),
        ];

        let compacted = compress_path(original.clone()).unwrap();
        let decompressed = decompress_path(compacted);
        let after_roundtrip = path_to_json(decompressed);

        assert_eq!(original, after_roundtrip);
    }

    #[test]
    fn test_encode_decode_event() {
        let evt = r#"{"rx": 0, "ry": 0, "_id": "c900cf81-8af6-4eb2-82fe-4be2b2eab5b9", "top": 265.76, "fill": "rgba(255,255,255,1)", "left": 440.13, "type": "rect", "angle": 0, "flipX": false, "flipY": false, "skewX": 0, "skewY": 0, "width": 398.96, "_order": -1, "height": 133.43, "scaleX": 1, "scaleY": 1, "shadow": null, "stroke": "rgba(255,255,255,1)", "opacity": 1, "originX": "left", "originY": "top", "version": "4.6.0", "visible": true, "fillRule": "nonzero", "_noHistory": null, "paintFirst": "fill", "strokeWidth": 2, "noScaleCache": false, "_lockedbyuser": null, "strokeLineCap": "butt", "strokeUniform": true, "_drawByStretch": true, "strokeLineJoin": "miter", "backgroundColor": "", "strokeDashArray": null, "strokeDashOffset": 0, "strokeMiterLimit": 4, "globalCompositeOperation": "source-over"}"#;
        let evt = serde_json::from_str(evt).unwrap();
        let evt = CompactEvent::from_json(evt).unwrap();
        match &evt {
            CompactEvent::Other(schema) => {
                assert_eq!(schema.kind, Kind::Rect);
                assert_eq!(schema._order, Some(-1));
            }
            CompactEvent::Path(_) => unreachable!("should be rect"),
        }

        let postcard_binary = postcard::to_allocvec(&evt).unwrap();
        let evt: CompactEvent = postcard::from_bytes(&postcard_binary).unwrap();
        match &evt {
            CompactEvent::Other(schema) => {
                assert_eq!(schema.kind, Kind::Rect);
                assert_eq!(schema._order, Some(-1));
            }
            CompactEvent::Path(_) => unreachable!("should be rect"),
        }

        let evt = "0110c900cf818af64eb282fe4be2b2eab5b90002cd4c85438f02e243146e0543e17ac7430000000000000000000000000000000000000000f03f000000000000f03f011372676261283235352c3235352c3235352c31290000011372676261283235352c3235352c3235352c3129010100000100010000000000000101040101020005342e362e3002000000000000010000010100000000000100000000010000000000000000000000000000000000000000000000000000000000";
        let evt = hex::decode(evt).unwrap();
        let evt: CompactEvent = postcard::from_bytes(&evt).unwrap();
        match &evt {
            CompactEvent::Other(schema) => {
                assert_eq!(schema.kind, Kind::Rect);
                assert_eq!(schema._order, Some(0));
            }
            CompactEvent::Path(_) => unreachable!("should be rect"),
        }

        let evt = "0110c900cf818af64eb282fe4be2b2eab5b90002cd4c85438f02e243146e0543e17ac7430000000000000000000000000000000000000000f03f000000000000f03f011372676261283235352c3235352c3235352c31290000011372676261283235352c3235352c3235352c3129010100000100010000000000000101040101020005342e362e3002000000000000010100010100000000000100000000010000000000000000000000000000000000000000000000000000000000";
        let evt = hex::decode(evt).unwrap();
        let evt: CompactEvent = postcard::from_bytes(&evt).unwrap();
        match &evt {
            CompactEvent::Other(schema) => {
                assert_eq!(schema.kind, Kind::Rect);
                assert_eq!(schema._order, Some(-1));
            }
            CompactEvent::Path(_) => unreachable!("should be rect"),
        }

        let evt = "0110a92b43303a5c4a9c9f98ad968837f0fe0002a470414385eb9e4300000243000002430000000000000000000000000000000000000000f03f000000000000f03f010d7267626128302c302c302c31290000010d7267626128302c302c302c3129010100000100010000000000000101040101020005342e362e300800000001010000010c0000000000000000000100008242010000000001db0fc94000000000000000000000000000000000000000000000";
        let evt = hex::decode(evt).unwrap();
        let evt: CompactEvent = postcard::from_bytes(&evt).unwrap();
        println!("{evt:#?}");
        let evt = evt.into_json().unwrap();
        println!("{evt:#?}");
    }

    #[test]
    fn test_losing_precision_error() {
        let evt = r#"{"type":"path","version":"4.6.0","originX":"left","originY":"top","left":541.18,"top":61.47,"width":97.2,"height":97.2,"fill":"rgba(255,255,255,1)","stroke":"rgba(255,255,255,1)","strokeWidth":2,"strokeDashArray":null,"strokeLineCap":"butt","strokeDashOffset":0,"strokeLineJoin":"miter","strokeUniform":true,"strokeMiterLimit":40,"scaleX":0.07,"scaleY":0.11,"angle":0,"flipX":false,"flipY":false,"opacity":1,"shadow":null,"visible":true,"backgroundColor":"","fillRule":"nonzero","paintFirst":"fill","globalCompositeOperation":"source-over","skewX":0,"skewY":0,"_id":"a52b6755-2c6e-452b-a1b8-2cc108da7f34","noScaleCache":false,"_order":171,"_noHistory":true,"_drawByStretch":true,"path":[["M",0,0],["L",97.2,0],["L",0,97.2],["z"]]}"#;
        let evt = serde_json::from_str(evt).unwrap();
        let evt = CompactEvent::from_json(evt).unwrap();
        match &evt {
            CompactEvent::Path(p) => assert_eq!(p.path.len(), 4),
            CompactEvent::Other(_) => unreachable!(),
        }
    }
}
