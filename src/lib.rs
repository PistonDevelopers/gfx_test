//! A library for drawing text for gfx-rs graphics API.
//! Uses freetype-rs underneath to former the font bitmap texture and collect
//! information about face glyphs.
//!
//! # Examples
//!
//! Basic usage:
//!
//! ```ignore
//! // Initialize text renderer.
//! let mut text = gfx_text::new(factory).build().unwrap();
//!
//! // In render loop:
//!
//! // Add some text 10 pixels down and right from the top left screen corner.
//! text.add(
//!     "The quick brown fox jumps over the lazy dog",  // Text to add
//!     [10, 10],                                       // Position
//!     [0.65, 0.16, 0.16, 1.0],                        // Text color
//! );
//!
//! // Draw text.
//! text.draw(&mut stream);
//! ```

#![deny(missing_docs)]

// #[macro_use]
// extern crate log;
#[macro_use]
extern crate gfx;
extern crate freetype;

use std::cmp::max;
use std::marker::PhantomData;
use gfx::{Factory, Resources, PrimitiveType, ProgramError, DrawError, UpdateError};
use gfx::traits::{FactoryExt, Output, Stream, ToIndexSlice, ToSlice};
use gfx::handle::{Program, Buffer, Texture};
use gfx::batch::Full as FullBatch;
use gfx::batch::Error as BatchError;
use gfx::tex::{self, TextureError};
mod font;
use font::BitmapFont;
pub use font::FontError;

const DEFAULT_FONT_SIZE: u8 = 16;
const DEFAULT_BUFFER_SIZE: usize = 128;
const DEFAULT_OUTLINE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const DEFAULT_PROJECTION: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[cfg(feature = "include-font")]
const DEFAULT_FONT_DATA: Option<&'static [u8]> =
    Some(include_bytes!("../assets/NotoSans-Regular.ttf"));
#[cfg(not(feature = "include-font"))]
const DEFAULT_FONT_DATA: Option<&'static [u8]> =
    None;

/// General error type returned by the library. Wraps all other errors.
#[derive(Debug)]
pub enum Error {
    /// Program linking error
    ProgramError(ProgramError),
    /// Font loading error
    FontError(FontError),
    /// Texture creation/updation error
    TextureError(TextureError),
    /// Draw-time error
    DrawError(DrawError<BatchError>),
    /// An error occuring at batch creation
    BatchError(BatchError),
    /// An error occuring in buffer/texture updates
    UpdateError(UpdateError<usize>),
}

/// An anchor aligns text horizontally to its given x position.
#[derive(PartialEq)]
pub enum HorizontalAnchor {
    /// Anchor the left edge of the text
    Left,
    /// Anchor the horizontal mid-point of the text
    Center,
    /// Anchor the right edge of the text
    Right,
}

/// An anchor aligns text vertically to its given y position.
#[derive(PartialEq)]
pub enum VerticalAnchor {
    /// Anchor the top edge of the text
    Top,
    /// Anchor the vertical mid-point of the text
    Center,
    /// Anchor the bottom edge of the text
    Bottom,
}

impl From<ProgramError> for Error {
    fn from(e: ProgramError) -> Error { Error::ProgramError(e) }
}

impl From<FontError> for Error {
    fn from(e: FontError) -> Error { Error::FontError(e) }
}

impl From<TextureError> for Error {
    fn from(e: TextureError) -> Error { Error::TextureError(e) }
}

impl From<DrawError<BatchError>> for Error {
    fn from(e: DrawError<BatchError>) -> Error { Error::DrawError(e) }
}

impl From<BatchError> for Error {
    fn from(e: BatchError) -> Error { Error::BatchError(e) }
}

impl From<UpdateError<usize>> for Error {
    fn from(e: UpdateError<usize>) -> Error { Error::UpdateError(e) }
}

type IndexT = u32;

/// Text renderer.
pub struct Renderer<R: Resources, F: Factory<R>> {
    factory: F,
    program: Program<R>,
    draw_state: gfx::DrawState,
    vertex_data: Vec<Vertex>,
    vertex_buffer: Buffer<R, Vertex>,
    index_data: Vec<IndexT>,
    index_buffer: Buffer<R, IndexT>,
    font_bitmap: BitmapFont,
    params: ShaderParams<R>,
}

/// Text renderer builder. Allows to set rendering options using builder
/// pattern.
///
/// # Examples
///
/// ```ignore
/// let mut text = gfx_text::RendererBuilder::new(factory)
///     .with_size(25)
///     .with_font("/path/to/font.ttf")
///     .with_chars(&['a', 'b', 'c'])
///     .build()
///     .unwrap();
/// ```
pub struct RendererBuilder<'r, R: Resources, F: Factory<R>> {
    factory: F,
    font_size: u8,
    // NOTE(Kagami): Better to use `P: AsRef<OsStr>` but since we store path in
    // the intermediate builder structure, Rust will unable to infer type
    // without manual annotation which is much worse. Anyway, it's possible to
    // just pass raw bytes.
    font_path: Option<&'r str>,
    font_data: Option<&'r [u8]>,
    outline_width: Option<u8>,
    outline_color: [f32; 4],
    buffer_size: usize,
    chars: Option<&'r [char]>,
    // XXX(Kagami): Shut up the Rust complains about unused R. We can't use
    // just `factory: &mut Factory<R>` because it doesn't work with lifetimes
    // (complains about the Marker associated type). Is there any better way?
    _r: PhantomData<R>,
}

/// Create a new text renderer builder. Alias for `RendererBuilder::new`.
pub fn new<'r, R: Resources, F: Factory<R>>(factory: F) -> RendererBuilder<'r, R, F> {
    RendererBuilder::new(factory)
}

impl<'r, R: Resources, F: Factory<R>> RendererBuilder<'r, R, F> {
    /// Create a new text renderer builder.
    pub fn new(factory: F) -> Self {
        // Default renderer settings.
        RendererBuilder {
            factory: factory,
            font_size: DEFAULT_FONT_SIZE,
            font_path: None,  // Default font will be used
            font_data: DEFAULT_FONT_DATA,
            outline_width: None,  // No outline by default
            outline_color: DEFAULT_OUTLINE_COLOR,
            buffer_size: DEFAULT_BUFFER_SIZE,
            chars: None,  // Place all available font chars into texture
            _r: PhantomData,
        }
    }

    /// Specify custom size.
    pub fn with_size(mut self, size: u8) -> Self {
        self.font_size = size;
        self
    }

    /// Specify custom font by path.
    pub fn with_font(mut self, path: &'r str) -> Self {
        self.font_path = Some(path);
        self
    }

    /// Pass raw font data.
    pub fn with_font_data(mut self, data: &'r [u8]) -> Self {
        self.font_data = Some(data);
        self
    }

    /// Specify outline width and color.
    /// **Not implemented yet.**
    pub fn with_outline(mut self, width: u8, color: [f32; 4]) -> Self {
        self.outline_width = Some(width);
        self.outline_color = color;
        self
    }

    /// Specify custom initial buffer size.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Make available only provided characters in font texture instead of
    /// loading all existing from the font face.
    pub fn with_chars(mut self, chars: &'r [char]) -> Self {
        self.chars = Some(chars);
        self
    }

    /// Build a new text renderer instance using current settings.
    pub fn build(mut self) -> Result<Renderer<R, F>, Error> {
        let program = try!(self.factory.link_program(VERTEX_SRC, FRAGMENT_SRC));
        let state = gfx::DrawState::new().blend(gfx::BlendPreset::Alpha);
        let vertex_buffer = self.factory.create_buffer_dynamic(
            self.buffer_size,
            gfx::BufferRole::Vertex,
        );
        let index_buffer = self.factory.create_buffer_dynamic(
            self.buffer_size,
            gfx::BufferRole::Index
        );

        // Initialize bitmap font.
        // TODO(Kagami): Outline!
        // TODO(Kagami): More granulated font settings, e.g. antialiasing,
        // hinting, kerning, etc.
        let font_bitmap = try!(match self.font_path {
            Some(path) =>
                BitmapFont::from_path(path, self.font_size, self.chars),
            None => match self.font_data {
                Some(data) => BitmapFont::from_bytes(data, self.font_size, self.chars),
                None => Err(FontError::NoFont),
            },
        });
        let font_texture = try!(create_texture_r8_static(
            &mut self.factory,
            font_bitmap.get_width(),
            font_bitmap.get_height(),
            font_bitmap.get_image(),
        ));
        let sampler = self.factory.create_sampler(
            tex::SamplerInfo::new(tex::FilterMethod::Bilinear,
                                  tex::WrapMode::Clamp)
        );

        Ok(Renderer {
            factory: self.factory,
            program: program,
            draw_state: state,
            vertex_data: Vec::new(),
            vertex_buffer: vertex_buffer,
            index_data: Vec::new(),
            index_buffer: index_buffer,
            font_bitmap: font_bitmap,
            params: ShaderParams {
                color: (font_texture, Some(sampler)),
                screen_size: [0.0, 0.0],
                proj: DEFAULT_PROJECTION,
                _r: PhantomData,
            },
        })
    }

    /// Just an alias for `builder.build().unwrap()`.
    pub fn unwrap(self) -> Renderer<R, F> {
        self.build().unwrap()
    }
}

impl<R: Resources, F: Factory<R>> Renderer<R, F> {
    /// Add some text to the current draw scene relative to the top left corner
    /// of the screen using pixel coordinates.
    pub fn add(&mut self, text: &str, pos: [i32; 2], color: [f32; 4]) {
        self.add_generic(text, Ok(pos), color)
    }

    /// Add text to the draw scene by anchoring an edge or mid-point to a
    /// position defined in screen pixel coordinates.
    pub fn add_anchored(&mut self, text: &str, pos: [i32; 2], horizontal: HorizontalAnchor, vertical: VerticalAnchor, color: [f32; 4]) {
        if horizontal == HorizontalAnchor::Left && vertical == VerticalAnchor::Top {
            self.add_generic(text, Ok(pos), color);
            return
        }

        let (width, height) = self.measure(text);
        let x = match horizontal {
            HorizontalAnchor::Left => pos[0],
            HorizontalAnchor::Center => pos[0] - width / 2,
            HorizontalAnchor::Right => pos[0] - width,
        };
        let y = match vertical {
            VerticalAnchor::Top => pos[1],
            VerticalAnchor::Center => pos[1] - height / 2,
            VerticalAnchor::Bottom => pos[1] - height,
        };

        self.add_generic(text, Ok([x, y]), color)
    }

    /// Add some text to the draw scene using absolute world coordinates.
    pub fn add_at(&mut self, text: &str, pos: [f32; 3], color: [f32; 4]) {
        self.add_generic(text, Err(pos), color)
    }

    fn add_generic(&mut self, text: &str, pos: Result<[i32; 2], [f32; 3]>, color: [f32; 4]) {
        // `Result` is used here as an `Either` analogue.
        let (screen_pos, world_pos, screen_rel) = match pos {
            Ok(screen_pos) => (screen_pos, [0.0, 0.0, 0.0], 1),
            Err(world_pos) => ([0, 0], world_pos, 0),
        };
        let (mut x, y) = (screen_pos[0] as f32, screen_pos[1] as f32);
        for ch in text.chars() {
            let ch_info = match self.font_bitmap.find_char(ch) {
                Some(info) => info,
                // Skip unknown chars from text string. Probably it would be
                // better to place some "?" mark instead but it may not exist
                // in the font too.
                None => continue,
            };
            let x_offset = x + ch_info.x_offset as f32;
            let y_offset = y + ch_info.y_offset as f32;
            let tex = ch_info.tex;
            let index = self.vertex_data.len() as u32;

            // Top-left point, index + 0.
            self.vertex_data.push(Vertex {
                pos: [x_offset, y_offset],
                tex: [tex[0], tex[1]],
                world_pos: world_pos,
                screen_rel: screen_rel,
                color: color,
            });
            // Bottom-left point, index + 1.
            self.vertex_data.push(Vertex {
                pos: [x_offset, y_offset + ch_info.height as f32],
                tex: [tex[0], tex[1] + ch_info.tex_height],
                world_pos: world_pos,
                screen_rel: screen_rel,
                color: color,
            });
            // Bottom-right point, index + 2.
            self.vertex_data.push(Vertex {
                pos: [x_offset + ch_info.width as f32, y_offset + ch_info.height as f32],
                tex: [tex[0] + ch_info.tex_width, tex[1] + ch_info.tex_height],
                world_pos: world_pos,
                screen_rel: screen_rel,
                color: color,
            });
            // Top-right point, index + 3.
            self.vertex_data.push(Vertex {
                pos: [x_offset + ch_info.width as f32, y_offset],
                tex: [tex[0] + ch_info.tex_width, tex[1]],
                world_pos: world_pos,
                screen_rel: screen_rel,
                color: color,
            });

            // Top-left triangle.
            // 0--3
            // | /
            // |/
            // 1
            self.index_data.push(index + 0);
            self.index_data.push(index + 1);
            self.index_data.push(index + 3);
            // Bottom-right triangle.
            //    3
            //   /|
            //  / |
            // 1--2
            self.index_data.push(index + 3);
            self.index_data.push(index + 1);
            self.index_data.push(index + 2);

            x += ch_info.x_advance as f32;
        }
    }

    /// Draw the current scene and clear state.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text.add("Test1", [10, 10], [1.0, 0.0, 0.0, 1.0]);
    /// text.add("Test2", [20, 20], [0.0, 1.0, 0.0, 1.0]);
    /// text.draw(&mut stream);
    /// ```
    pub fn draw<S: Stream<R>>(&mut self, stream: &mut S) -> Result<(), Error> {
        self.draw_at(stream, DEFAULT_PROJECTION)
    }

    /// Draw using provided projection matrix.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text.add_at("Test1", [6.0, 0.0, 0.0], [1.0, 0.0, 0.0, 1.0]);
    /// text.add_at("Test2", [0.0, 5.0, 0.0], [0.0, 1.0, 0.0, 1.0]);
    /// text.draw_at(&mut stream, camera_projection);
    /// ```
    pub fn draw_at<S: Stream<R>>(
        &mut self,
        stream: &mut S,
        proj: [[f32; 4]; 4]
    ) -> Result<(), Error> {
        let ver_len = self.vertex_data.len();
        let ver_buf_len = self.vertex_buffer.len();
        let ind_len = self.index_data.len();
        let ind_buf_len = self.index_buffer.len();
        // Reallocate buffers if there is no enough space for data.
        if ver_len > ver_buf_len {
            self.vertex_buffer = self.factory.create_buffer_dynamic(
                grow_buffer_size(ver_buf_len, ver_len),
                gfx::BufferRole::Vertex
            );
        }
        if ind_len > ind_buf_len {
            let len = grow_buffer_size(ind_buf_len, ind_len);
            self.index_buffer = self.factory.create_buffer_dynamic(len, gfx::BufferRole::Index);
        }
        // Move vertex/index data.
        {
            let renderer = stream.access().0;
            try!(renderer.update_buffer(self.vertex_buffer.raw(), &self.vertex_data, 0));
            try!(renderer.update_buffer(self.index_buffer.raw(), &self.index_data, 0));
        }
        let nv = self.vertex_data.len() as gfx::VertexCount;
        let ni = self.index_data.len() as gfx::VertexCount;

        // Clear state.
        self.vertex_data.clear();
        self.index_data.clear();

        let mesh = gfx::Mesh::from_format(self.vertex_buffer.clone(), nv);
        let mut slice = self.index_buffer.to_slice(PrimitiveType::TriangleList);
        slice.end = ni;
        self.params.screen_size = {
            let (w, h) = stream.get_output().get_size();
            [w as f32, h as f32]
        };
        self.params.proj = proj;
        let batch = gfx::batch::bind(
            &self.draw_state,
            &mesh,
            slice,
            &self.program,
            &self.params);

        Ok(try!(stream.draw(&batch)))
    }

    /// Former resulting batch.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let batch = text.get_batch(stream.get_output());
    /// stream.draw(&batch);
    /// ```
    pub fn get_batch<O: Output<R>>(&mut self, output: &O)
                     -> Result<FullBatch<ShaderParams<R>>, Error> {
        self.get_batch_at(output, DEFAULT_PROJECTION)
    }

    /// Former batch for the given projection matrix.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let batch = text.get_batch_at(stream.get_output(), camera_projection);
    /// stream.draw(&batch);
    /// ```
    pub fn get_batch_at<O: Output<R>>(&mut self, output: &O, proj: [[f32; 4]; 4])
                        -> Result<FullBatch<ShaderParams<R>>, Error> {
        let mesh = self.factory.create_mesh(&self.vertex_data);
        let slice = self.index_data.to_slice(&mut self.factory,
                                             PrimitiveType::TriangleList);
        self.vertex_data.clear();
        self.index_data.clear();
        self.params.screen_size = {
            let (w, h) = output.get_size();
            [w as f32, h as f32]
        };
        self.params.proj = proj;
        let mut batch = try!(FullBatch::new(mesh, self.program.clone(),
                                            self.params.clone()));
        batch.slice = slice;
        Ok(batch)
    }

    // TODO: Currently reports height based on the tallest glyph in the string.
    // It might be more useful to go by the tallest in the whole font to avoid
    // text jumping around as it changes.
    /// Get the bounding box size of a string as rendered by this font.
    pub fn measure(&self, text: &str) -> (i32, i32) {
        let mut width = 0;
        let mut height = 0;
        let mut last_char = None;

        for ch in text.chars() {
            let ch_info = match self.font_bitmap.find_char(ch) {
                Some(info) => info,
                None => continue,
            };
            last_char = Some(ch_info);

            width += ch_info.x_advance;
            height = max(height, ch_info.y_offset + ch_info.height);
        }

        match last_char {
            Some(info) => width += info.x_offset + info.width - info.x_advance,
            None => (),
        }

        (width, height)
    }
}

// Some missing helpers.

fn grow_buffer_size(mut current_size: usize, desired_size: usize) -> usize {
    if current_size < 1 {
        current_size = 1;
    }
    while current_size < desired_size {
        current_size *= 2;
    }
    current_size
}

fn create_texture_r8_static<R: Resources, F: Factory<R>>(
    factory: &mut F,
    width: u16,
    height: u16,
    data: &[u8],
) -> Result<Texture<R>, TextureError> {
    let texture = try!(factory.create_texture(tex::TextureInfo {
        width: width,
        height: height,
        depth: 1,
        levels: 1,
        kind: tex::Kind::D2,
        format: tex::R8,
    }));
    try!(factory.update_texture_raw(
        &texture,
        &(*texture.get_info()).into(),
        data,
        None,
    ));
    Ok(texture)
}

// Hack to hide shader structs from the library user.
mod shader_structs {
    use gfx::shade::TextureParam;

    gfx_vertex!( Vertex {
        a_Pos@ pos: [f32; 2],
        a_TexCoord@ tex: [f32; 2],
        a_World_Pos@ world_pos: [f32; 3],
        // Should be bool but gfx-rs doesn't support it.
        a_Screen_Rel@ screen_rel: i32,
        a_Color@ color: [f32; 4],
    });

    gfx_parameters!( ShaderParams {
        t_Color@ color: TextureParam<R>,
        u_Screen_Size@ screen_size: [f32; 2],
        u_Proj@ proj: [[f32; 4]; 4],
    });
}
use shader_structs::{Vertex, ShaderParams};

const VERTEX_SRC: &'static [u8] = b"
    #version 130

    in vec2 a_Pos;
    in vec4 a_Color;
    in vec2 a_TexCoord;
    in vec4 a_World_Pos;
    in int a_Screen_Rel;
    out vec4 v_Color;
    out vec2 v_TexCoord;
    uniform vec2 u_Screen_Size;
    uniform mat4 u_Proj;

    void main() {
        // On-screen offset from text origin.
        vec2 v_Screen_Offset = vec2(
            2 * a_Pos.x / u_Screen_Size.x - 1,
            1 - 2 * a_Pos.y / u_Screen_Size.y
        );
        vec4 v_Screen_Pos = u_Proj * a_World_Pos;
        vec2 v_World_Offset = a_Screen_Rel == 0
            // Perspective divide to get normalized device coords.
            ? vec2 (
                v_Screen_Pos.x / v_Screen_Pos.z + 1,
                v_Screen_Pos.y / v_Screen_Pos.z - 1
            ) : vec2(0.0, 0.0);

        v_Color = a_Color;
        v_TexCoord = a_TexCoord;
        gl_Position = vec4(v_World_Offset + v_Screen_Offset, 0.0, 1.0);
    }
";

const FRAGMENT_SRC: &'static [u8] = b"
    #version 130

    in vec4 v_Color;
    in vec2 v_TexCoord;
    out vec4 o_Color;
    uniform sampler2D t_Color;

    void main() {
        vec4 t_Font_Color = texture(t_Color, v_TexCoord);
        o_Color = vec4(v_Color.rgb, t_Font_Color.r * v_Color.a);
    }
";
