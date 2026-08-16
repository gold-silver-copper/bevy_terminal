use std::{io, ops::Range};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};

#[derive(Debug, Clone)]
pub struct ParleyBackend {
    buffer: Buffer,
    cursor_position: Position,
    cursor_visible: bool,
}

impl ParleyBackend {
    pub fn new(width: u16, height: u16) -> Self {
        let area = Rect::new(0, 0, width, height);
        Self {
            buffer: Buffer::empty(area),
            cursor_position: Position::ORIGIN,
            cursor_visible: true,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn cursor_position(&self) -> Position {
        self.cursor_position
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.buffer.resize(Rect::new(0, 0, width, height));
    }
}

impl Backend for ParleyBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if x < self.buffer.area.width && y < self.buffer.area.height {
                self.buffer[(x, y)].clone_from(cell);
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor_position)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor_position = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.buffer.reset();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let area = self.buffer.area;
        match clear_type {
            ClearType::All => self.clear(),
            ClearType::AfterCursor => {
                for y in self.cursor_position.y..area.height {
                    let start_x = if y == self.cursor_position.y {
                        self.cursor_position.x.saturating_add(1)
                    } else {
                        0
                    };
                    for x in start_x..area.width {
                        self.buffer[(x, y)].reset();
                    }
                }
                Ok(())
            }
            ClearType::BeforeCursor => {
                for y in 0..=self.cursor_position.y.min(area.height.saturating_sub(1)) {
                    let end_x = if y == self.cursor_position.y {
                        self.cursor_position.x
                    } else {
                        area.width
                    };
                    for x in 0..end_x.min(area.width) {
                        self.buffer[(x, y)].reset();
                    }
                }
                Ok(())
            }
            ClearType::CurrentLine => {
                if self.cursor_position.y < area.height {
                    for x in 0..area.width {
                        self.buffer[(x, self.cursor_position.y)].reset();
                    }
                }
                Ok(())
            }
            ClearType::UntilNewLine => {
                if self.cursor_position.y < area.height {
                    for x in self.cursor_position.x..area.width {
                        self.buffer[(x, self.cursor_position.y)].reset();
                    }
                }
                Ok(())
            }
        }
    }

    fn scroll_region_up(&mut self, region: Range<u16>, line_count: u16) -> io::Result<()> {
        let start = region.start.min(self.buffer.area.height);
        let end = region.end.min(self.buffer.area.height).max(start);
        let count = line_count.min(end - start);
        let width = self.buffer.area.width;
        for y in start..end.saturating_sub(count) {
            for x in 0..width {
                self.buffer[(x, y)] = self.buffer[(x, y + count)].clone();
            }
        }
        for y in end.saturating_sub(count)..end {
            for x in 0..width {
                self.buffer[(x, y)].reset();
            }
        }
        Ok(())
    }

    fn scroll_region_down(
        &mut self,
        region: Range<u16>,
        line_count: u16,
    ) -> io::Result<()> {
        let start = region.start.min(self.buffer.area.height);
        let end = region.end.min(self.buffer.area.height).max(start);
        let count = line_count.min(end - start);
        let width = self.buffer.area.width;
        for y in (start + count..end).rev() {
            for x in 0..width {
                self.buffer[(x, y)] = self.buffer[(x, y - count)].clone();
            }
        }
        for y in start..start + count {
            for x in 0..width {
                self.buffer[(x, y)].reset();
            }
        }
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(self.buffer.area.width, self.buffer.area.height))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size()?,
            pixels: Size::new(self.buffer.area.width, self.buffer.area.height),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
