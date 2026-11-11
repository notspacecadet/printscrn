//! Annotation document. Coordinates are crop pixels (physical), origin top-left.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }
}

pub const SWATCHES: [Color; 8] = [
    Color::rgb(255, 59, 48),
    Color::rgb(255, 159, 10),
    Color::rgb(255, 214, 10),
    Color::rgb(48, 209, 88),
    Color::rgb(10, 132, 255),
    Color::rgb(191, 90, 242),
    Color::rgb(255, 255, 255),
    Color::rgb(28, 28, 30),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pen,
    Marker,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Text,
    Blur,
    Pixelate,
    Eyedropper,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Pen => "Pen",
            Self::Marker => "Marker",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
            Self::Rect => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Text => "Text",
            Self::Blur => "Blur",
            Self::Pixelate => "Pixelate",
            Self::Eyedropper => "Eyedropper",
        }
    }

    pub fn shortcut(self) -> Option<char> {
        Some(match self {
            Self::Select => return None,
            Self::Pen => 'P',
            Self::Marker => 'M',
            Self::Line => 'L',
            Self::Arrow => 'A',
            Self::Rect => 'R',
            Self::Ellipse => 'O',
            Self::Text => 'T',
            Self::Blur => 'B',
            Self::Pixelate => 'X',
            Self::Eyedropper => 'I',
        })
    }

    pub fn draws(self) -> bool {
        !matches!(self, Self::Select | Self::Eyedropper | Self::Text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Pen { points: Vec<Pt> },
    Marker { points: Vec<Pt> },
    Line { a: Pt, b: Pt },
    Arrow { a: Pt, b: Pt },
    Rect { a: Pt, b: Pt },
    Ellipse { a: Pt, b: Pt },
    Text { at: Pt, text: String, size: f32 },
    Blur { a: Pt, b: Pt, radius: f32 },
    Pixelate { a: Pt, b: Pt, block: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub shape: Shape,
    pub color: Color,
    pub stroke: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Document {
    items: Vec<Annotation>,
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
}

impl Document {
    pub fn items(&self) -> &[Annotation] {
        &self.items
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn push(&mut self, item: Annotation) {
        self.undo.push(self.items.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.items.push(item);
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.items, prev));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.items, next));
        true
    }

    pub fn clear(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.undo.push(std::mem::take(&mut self.items));
        self.redo.clear();
    }
}

/// Drop points that sit on top of each other so a slow pen stroke stays cheap.
pub fn simplify_stroke(points: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(points.len());
    for point in points {
        if let Some(prev) = out.last() {
            let dx = point.x - prev.x;
            let dy = point.y - prev.y;
            if dx * dx + dy * dy < 1.0 {
                continue;
            }
        }
        out.push(*point);
    }
    if let (Some(last), Some(kept)) = (points.last(), out.last()) {
        if (last.x - kept.x).abs() > 0.01 || (last.y - kept.y).abs() > 0.01 {
            out.push(*last);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot() -> Annotation {
        Annotation {
            shape: Shape::Pen {
                points: vec![Pt { x: 1.0, y: 1.0 }],
            },
            color: Color::rgb(1, 2, 3),
            stroke: 2.0,
        }
    }

    #[test]
    fn undo_and_redo_roundtrip() {
        let mut doc = Document::default();
        assert!(!doc.undo());
        doc.push(dot());
        doc.push(dot());
        assert_eq!(doc.items().len(), 2);
        assert!(doc.undo());
        assert_eq!(doc.items().len(), 1);
        assert!(doc.redo());
        assert_eq!(doc.items().len(), 2);
        doc.undo();
        doc.push(dot());
        assert!(!doc.can_redo(), "a new mark clears the redo stack");
    }

    #[test]
    fn simplify_keeps_endpoints() {
        let points = vec![
            Pt { x: 0.0, y: 0.0 },
            Pt { x: 0.2, y: 0.1 },
            Pt { x: 5.0, y: 0.0 },
        ];
        let simple = simplify_stroke(&points);
        assert_eq!(simple.first().unwrap().x, 0.0);
        assert_eq!(simple.last().unwrap().x, 5.0);
        assert!(simple.len() < points.len());
    }
}
