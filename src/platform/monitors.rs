//! Current desktop work areas, converted from physical pixels to egui coordinates.
use eframe::egui;
pub fn areas(pixels_per_point: f32) -> Option<Vec<egui::Rect>> {
    let scale = pixels_per_point.max(0.25);
    let areas = native()?;
    let result: Vec<_> = areas
        .into_iter()
        .filter(|r| r[2] > 0 && r[3] > 0)
        .map(|r| {
            egui::Rect::from_min_size(
                egui::pos2(r[0] as f32 / scale, r[1] as f32 / scale),
                egui::vec2(r[2] as f32 / scale, r[3] as f32 / scale),
            )
        })
        .collect();
    (!result.is_empty()).then_some(result)
}
#[cfg(target_os = "windows")]
fn native() -> Option<Vec<[i32; 4]>> {
    use windows::Win32::{
        Foundation::{LPARAM, RECT},
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
    };
    use windows::core::BOOL;
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: EnumDisplayMonitors synchronously calls us with a live monitor and
        // the exact vector pointer supplied below, never retained after the call.
        unsafe {
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                let r = info.rcWork;
                (&mut *(data.0 as *mut Vec<[i32; 4]>)).push([
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                ]);
            }
        }
        BOOL(1)
    }
    let mut areas = Vec::new();
    // SAFETY: callback is synchronous, the vector outlives every callback.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut areas as *mut _ as isize),
        )
    }
    .as_bool();
    if !ok {
        return None;
    }
    Some(areas)
}
#[cfg(target_os = "linux")]
fn native() -> Option<Vec<[i32; 4]>> {
    use x11rb::{
        connection::Connection,
        protocol::{
            randr::ConnectionExt,
            xproto::{AtomEnum, ConnectionExt as _},
        },
    };
    // Wayland deliberately provides no global window positioning contract.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let monitors = connection
        .randr_get_monitors(root, true)
        .ok()?
        .reply()
        .ok()?;
    let atom = connection
        .intern_atom(true, b"_NET_WORKAREA")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let desktop_atom = connection
        .intern_atom(true, b"_NET_CURRENT_DESKTOP")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let desktop = connection
        .get_property(false, root, desktop_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .next()
        .unwrap_or(0);
    let work = connection
        .get_property(
            false,
            root,
            atom,
            AtomEnum::CARDINAL,
            desktop.saturating_mul(4),
            4,
        )
        .ok()?
        .reply()
        .ok()?;
    let work: Vec<_> = work
        .value32()
        .map(|values| values.collect())
        .unwrap_or_default();
    let usable = (work.len() == 4).then(|| {
        egui::Rect::from_min_size(
            egui::pos2(work[0] as i32 as f32, work[1] as i32 as f32),
            egui::vec2(work[2] as f32, work[3] as f32),
        )
    });
    Some(
        monitors
            .monitors
            .into_iter()
            .map(|m| {
                let full = egui::Rect::from_min_size(
                    egui::pos2(f32::from(m.x), f32::from(m.y)),
                    egui::vec2(f32::from(m.width), f32::from(m.height)),
                );
                let r = usable.map_or(full, |area| {
                    let intersection = area.intersect(full);
                    if intersection.is_positive() {
                        intersection
                    } else {
                        full
                    }
                });
                [
                    r.min.x as i32,
                    r.min.y as i32,
                    r.width() as i32,
                    r.height() as i32,
                ]
            })
            .collect(),
    )
}
