use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::RECT,
    Graphics::{Gdi::HDC, GdiPlus::*},
};

pub struct DrawingSession(usize);

impl DrawingSession {
    pub fn start() -> io::Result<Self> {
        let settings = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..GdiplusStartupInput::default()
        };
        let mut token = 0;
        // SAFETY: The initialized settings and output token live through initialization.
        let result = unsafe { GdiplusStartup(&mut token, &settings, null_mut()) };
        if result == 0 {
            Result::Ok(Self(token))
        } else {
            Err(io::Error::other(format!(
                "Falha ao inicializar o desenho (GDI+ {result})."
            )))
        }
    }
}

impl Drop for DrawingSession {
    fn drop(&mut self) {
        // SAFETY: This is the returned token. All graphics associated with it have been released.
        unsafe { GdiplusShutdown(self.0) };
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Back,
    Forward,
    Reload,
    Loading(f32),
    Download,
    Home,
    Star(bool),
    Plus,
    Close,
    Minimize,
    Maximize(bool),
    Menu,
    Folder,
    Shield,
    Chevron,
    Globe,
    Zoom,
    Mobile,
    DockLeft,
    DockBottom,
    DockRight,
    Undock,
    Pin,
}

pub struct Canvas(*mut GpGraphics);

impl Canvas {
    pub fn with_dc(dc: HDC, draw: impl FnOnce(&Self)) {
        let mut graphics = null_mut();
        // SAFETY: The caller lends the paint message's DC for this synchronous operation.
        if unsafe { GdipCreateFromHDC(dc, &mut graphics) } == 0 {
            let canvas = Self(graphics);
            // SAFETY: The canvas owns a live GDI+ graphics object until this scope ends.
            unsafe { GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias8x8) };
            draw(&canvas);
        }
    }

    pub fn rounded(&self, rect: RECT, radius: f32, color: u32) {
        let width = (rect.right - rect.left) as f32;
        let height = (rect.bottom - rect.top) as f32;
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let diameter = (2.0 * radius).min(width).min(height);
        let left = rect.left as f32;
        let top = rect.top as f32;
        let right = rect.right as f32 - diameter;
        let bottom = rect.bottom as f32 - diameter;
        self.with_brush(color, |brush| {
            let mut path = null_mut();
            // SAFETY: All GDI+ pointers here are owned by this call or borrowed from the canvas.
            unsafe {
                if GdipCreatePath(FillModeAlternate, &mut path) != 0 {
                    return;
                }
                GdipAddPathArc(path, left, top, diameter, diameter, 180.0, 90.0);
                GdipAddPathArc(path, right, top, diameter, diameter, 270.0, 90.0);
                GdipAddPathArc(path, right, bottom, diameter, diameter, 0.0, 90.0);
                GdipAddPathArc(path, left, bottom, diameter, diameter, 90.0, 90.0);
                GdipClosePathFigure(path);
                GdipFillPath(self.0, brush, path);
                GdipDeletePath(path);
            }
        });
    }

    pub fn dot(&self, x: f32, y: f32, size: f32, color: u32) {
        self.with_brush(color, |brush| {
            // SAFETY: Graphics and brush stay live for this draw call.
            unsafe { GdipFillEllipse(self.0, brush, x, y, size, size) };
        });
    }

    fn with_brush(&self, color: u32, draw: impl FnOnce(*mut GpBrush)) {
        let mut brush = null_mut();
        // SAFETY: On success GDI+ returns an owned brush. The draw callback only borrows it.
        unsafe {
            if GdipCreateSolidFill(argb(color), &mut brush) == 0 {
                draw(brush.cast());
                GdipDeleteBrush(brush.cast());
            }
        }
    }

    pub fn icon(&self, icon: Icon, x: f32, y: f32, size: f32, color: u32) {
        let scale = size / 20.0;
        let mut pen = null_mut();
        // SAFETY: Creates a pen owned by this function. The helpers below borrow it.
        if unsafe { GdipCreatePen1(argb(color), scale * 1.65, UnitPixel, &mut pen) } != 0 {
            return;
        }
        // SAFETY: Both settings apply to the live, owned pen.
        unsafe {
            GdipSetPenLineCap197819(pen, LineCapRound, LineCapRound, DashCapRound);
            GdipSetPenLineJoin(pen, LineJoinRound);
        }
        let line = |points: &[(f32, f32)]| {
            let points: Vec<_> = points
                .iter()
                .map(|&(px, py)| PointF {
                    X: x + px * scale,
                    Y: y + py * scale,
                })
                .collect();
            // SAFETY: The points, borrowed pen and graphics live for this synchronous call.
            unsafe { GdipDrawLines(self.0, pen, points.as_ptr(), points.len() as i32) };
        };
        match icon {
            Icon::Back | Icon::Forward => {
                let end = if matches!(icon, Icon::Back) {
                    4.0
                } else {
                    16.0
                };
                let start = 20.0 - end;
                line(&[(start, 10.0), (end, 10.0)]);
                line(&[(10.0, 4.0), (end, 10.0), (10.0, 16.0)]);
            }
            Icon::Reload => {
                // SAFETY: Graphics and pen are live and the coordinates are finite.
                unsafe {
                    GdipDrawArc(
                        self.0,
                        pen,
                        x + 4.0 * scale,
                        y + 4.0 * scale,
                        12.0 * scale,
                        12.0 * scale,
                        -18.0,
                        312.0,
                    )
                };
                line(&[(15.9, 3.5), (15.9, 7.5), (11.9, 7.5)]);
            }
            Icon::Loading(phase) => {
                // SAFETY: Graphics and pen are live and the coordinates are finite.
                unsafe {
                    GdipDrawArc(
                        self.0,
                        pen,
                        x + 3.5 * scale,
                        y + 3.5 * scale,
                        13.0 * scale,
                        13.0 * scale,
                        phase,
                        215.0,
                    )
                };
            }
            Icon::Download => {
                line(&[(10.0, 2.5), (10.0, 12.0)]);
                line(&[(6.0, 8.5), (10.0, 12.5), (14.0, 8.5)]);
                line(&[(3.4, 13.3), (3.4, 17.5), (16.6, 17.5), (16.6, 13.3)]);
            }
            Icon::Home => {
                line(&[(2.5, 9.2), (10.0, 3.2), (17.5, 9.2)]);
                line(&[(4.7, 8.0), (4.7, 16.8), (15.3, 16.8), (15.3, 8.0)]);
                line(&[(8.5, 16.8), (8.5, 12.2), (11.5, 12.2), (11.5, 16.8)]);
            }
            Icon::Star(filled) => {
                let shape = [
                    (10.0, 2.4),
                    (12.3, 7.3),
                    (17.7, 8.1),
                    (13.8, 11.8),
                    (14.8, 17.1),
                    (10.0, 14.6),
                    (5.2, 17.1),
                    (6.2, 11.8),
                    (2.3, 8.1),
                    (7.7, 7.3),
                    (10.0, 2.4),
                ];
                if filled {
                    let points: Vec<_> = shape
                        .iter()
                        .map(|&(px, py)| PointF {
                            X: x + px * scale,
                            Y: y + py * scale,
                        })
                        .collect();
                    self.with_brush(color, |brush| {
                        // SAFETY: The slice, brush and graphics are valid for the fill call.
                        unsafe {
                            GdipFillPolygon(
                                self.0,
                                brush,
                                points.as_ptr(),
                                points.len() as i32,
                                FillModeAlternate,
                            )
                        };
                    });
                } else {
                    line(&shape);
                }
            }
            Icon::Plus => {
                line(&[(4.5, 10.0), (15.5, 10.0)]);
                line(&[(10.0, 4.5), (10.0, 15.5)]);
            }
            Icon::Close => {
                line(&[(5.7, 5.7), (14.3, 14.3)]);
                line(&[(14.3, 5.7), (5.7, 14.3)]);
            }
            Icon::Minimize => line(&[(5.0, 10.0), (15.0, 10.0)]),
            Icon::Pin => {
                line(&[
                    (7.0, 3.5),
                    (13.0, 3.5),
                    (12.4, 8.9),
                    (15.0, 12.2),
                    (5.0, 12.2),
                    (7.6, 8.9),
                    (7.0, 3.5),
                ]);
                line(&[(10.0, 12.2), (10.0, 17.3)]);
            }
            Icon::Zoom => {
                // SAFETY: The drawing session and the pen both live through this call.
                unsafe {
                    GdipDrawEllipse(
                        self.0,
                        pen,
                        x + 3.0 * scale,
                        y + 3.0 * scale,
                        9.0 * scale,
                        9.0 * scale,
                    )
                };
                line(&[(11.0, 11.0), (17.0, 17.0)]);
            }
            Icon::Mobile => {
                line(&[(3.0, 13.0), (3.0, 4.0), (15.0, 4.0), (15.0, 6.0)]);
                line(&[(3.0, 13.0), (8.0, 13.0)]);
                line(&[(7.0, 16.0), (8.0, 13.0)]);
                line(&[
                    (11.0, 8.0),
                    (17.0, 8.0),
                    (17.0, 17.0),
                    (11.0, 17.0),
                    (11.0, 8.0),
                ]);
                line(&[(13.4, 14.7), (14.6, 14.7)]);
            }
            Icon::DockLeft | Icon::DockBottom | Icon::DockRight | Icon::Undock => {
                if matches!(icon, Icon::Undock) {
                    line(&[(3.0, 7.0), (3.0, 16.0), (13.0, 16.0)]);
                    line(&[
                        (6.0, 4.0),
                        (17.0, 4.0),
                        (17.0, 13.0),
                        (6.0, 13.0),
                        (6.0, 4.0),
                    ]);
                } else {
                    line(&[
                        (3.0, 4.0),
                        (17.0, 4.0),
                        (17.0, 16.0),
                        (3.0, 16.0),
                        (3.0, 4.0),
                    ]);
                    match icon {
                        Icon::DockLeft => line(&[(8.0, 4.0), (8.0, 16.0)]),
                        Icon::DockBottom => line(&[(3.0, 11.0), (17.0, 11.0)]),
                        _ => line(&[(12.0, 4.0), (12.0, 16.0)]),
                    }
                }
            }
            Icon::Maximize(restore) => {
                if restore {
                    line(&[
                        (7.5, 6.6),
                        (7.5, 4.6),
                        (15.3, 4.6),
                        (15.3, 12.5),
                        (13.4, 12.5),
                    ]);
                    line(&[
                        (5.0, 7.5),
                        (12.5, 7.5),
                        (12.5, 15.0),
                        (5.0, 15.0),
                        (5.0, 7.5),
                    ]);
                } else {
                    line(&[
                        (5.0, 5.0),
                        (15.0, 5.0),
                        (15.0, 15.0),
                        (5.0, 15.0),
                        (5.0, 5.0),
                    ]);
                }
            }
            Icon::Menu => {
                for py in [4.1, 9.0, 13.9] {
                    self.dot(x + 9.0 * scale, y + py * scale, 2.2 * scale, color);
                }
            }
            Icon::Folder => {
                line(&[
                    (3.0, 5.5),
                    (8.3, 5.5),
                    (10.4, 7.3),
                    (16.9, 7.3),
                    (16.9, 15.2),
                    (3.0, 15.2),
                    (3.0, 5.5),
                ]);
            }
            Icon::Shield => {
                line(&[
                    (10.0, 2.5),
                    (16.0, 5.0),
                    (16.0, 9.5),
                    (14.8, 13.7),
                    (10.0, 17.0),
                    (5.2, 13.7),
                    (4.0, 9.5),
                    (4.0, 5.0),
                    (10.0, 2.5),
                ]);
                line(&[(7.2, 9.7), (9.2, 11.6), (13.0, 7.7)]);
            }
            Icon::Chevron => line(&[(5.1, 8.0), (10.0, 12.8), (14.9, 8.0)]),
            Icon::Globe => {
                // SAFETY: The three ellipses use the live pen and graphics.
                unsafe {
                    GdipDrawEllipse(
                        self.0,
                        pen,
                        x + 3.1 * scale,
                        y + 3.1 * scale,
                        13.8 * scale,
                        13.8 * scale,
                    );
                    GdipDrawEllipse(
                        self.0,
                        pen,
                        x + 7.4 * scale,
                        y + 3.1 * scale,
                        5.2 * scale,
                        13.8 * scale,
                    );
                }
                line(&[(3.1, 10.0), (16.9, 10.0)]);
            }
        }
        // SAFETY: All drawing using this owned pen has finished.
        unsafe { GdipDeletePen(pen) };
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: This object owns the graphics wrapper. The Windows DC is still caller-owned.
        unsafe { GdipDeleteGraphics(self.0) };
    }
}

fn argb(color: u32) -> u32 {
    0xff00_0000 | ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}
