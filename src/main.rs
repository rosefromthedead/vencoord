use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_registry,
    delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyboardHandler, Keysym},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use std::{num::NonZeroUsize, ptr::NonNull, sync::Arc};
use vello::{
    glyph::{
        skrifa::{instance::Size, setting::VariationSetting, FontRef, MetadataProvider},
        Glyph,
    },
    peniko::{Blob, Color, Fill, Font},
    AaSupport, RenderParams,
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard::WlKeyboard, wl_output, wl_seat, wl_surface},
    Connection, Proxy, QueueHandle,
};

fn main() {
    tracing_subscriber::fmt().init();

    let font = Font::new(
        Blob::new(Arc::new(include_bytes!(
            "/usr/share/fonts/hack/Hack-Regular.ttf"
        ))),
        0,
    );
    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let layer_shell_state = LayerShell::bind(&globals, &qh).expect("yikes");

    let surface = compositor_state.create_surface(&qh);
    let window = layer_shell_state.create_layer_surface(
        &qh,
        surface.clone(),
        Layer::Overlay,
        None::<&'static str>,
        None,
    );
    window.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    window.commit();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let raw_display_handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
        NonNull::new(conn.backend().display_ptr() as *mut _).unwrap(),
    ));
    let raw_window_handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(
        NonNull::new(surface.id().as_ptr() as *mut _).unwrap(),
    ));

    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
            .unwrap()
    };

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .expect("Failed to find suitable adapter");

    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None))
        .expect("Failed to request device");

    let mut vencoord = Vencoord {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        keyboard_state: None,

        exit: false,
        width: 0,
        height: 0,
        window,

        device,
        surface,
        adapter,
        queue,

        font,
    };

    loop {
        event_queue.blocking_dispatch(&mut vencoord).unwrap();

        if vencoord.exit {
            break;
        }
    }

    // On exit we must destroy the surface before the window is destroyed.
    drop(vencoord.surface);
    drop(vencoord.window);
}

struct Vencoord {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    keyboard_state: Option<WlKeyboard>,

    exit: bool,
    width: u32,
    height: u32,
    window: LayerSurface,

    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,

    font: Font,
}

impl CompositorHandler for Vencoord {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }
}

impl OutputHandler for Vencoord {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for Vencoord {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        tracing::info!("configure {configure:?}");
        if (self.width, self.height) == configure.new_size {
            // why lol
            return;
        }
        let (new_width, new_height) = configure.new_size;
        self.width = new_width;
        self.height = new_height;

        let adapter = &self.adapter;
        let surface = &self.surface;

        let cap = surface.get_capabilities(&adapter);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: cap.formats[0],
            view_formats: vec![cap.formats[0]],
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            width: self.width,
            height: self.height,
            desired_maximum_frame_latency: 1,
            present_mode: wgpu::PresentMode::Mailbox,
        };

        surface.configure(&self.device, &surface_config);

        let mut renderer = vello::Renderer::new(
            &self.device,
            vello::RendererOptions {
                surface_format: Some(cap.formats[0]),
                use_cpu: false,
                antialiasing_support: AaSupport::all(),
                num_init_threads: Some(NonZeroUsize::new(1).unwrap()),
            },
        )
        .expect("whyyy");
        let mut scene = vello::Scene::new();
        let font = FontRef::new(self.font.data.as_ref()).unwrap();
        let font_size = 12f32;
        let axes = font.axes();
        let var_loc = axes.location::<[VariationSetting; 0]>([]);
        let metrics = font.metrics(Size::new(font_size), &var_loc);
        let glyph_metrics = font.glyph_metrics(Size::new(font_size), &var_loc);
        let charmap = font.charmap();
        let mut pen_x = 0f32;
        let pen_y = metrics.ascent - metrics.descent + metrics.leading;
        scene
            .draw_glyphs(&self.font)
            .hint(true)
            .brush(Color::RED)
            .font_size(font_size)
            .draw(
                Fill::NonZero,
                "hi".chars().map(|c| {
                    let gid = charmap.map(c).unwrap_or_default();
                    let advance = glyph_metrics.advance_width(gid).unwrap_or(0f32);
                    let x = pen_x;
                    pen_x += advance;
                    Glyph {
                        id: gid.to_u32(),
                        x,
                        y: pen_y,
                    }
                }),
            );

        let surface_texture = surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");
        renderer
            .render_to_surface(
                &self.device,
                &self.queue,
                &scene,
                &surface_texture,
                &RenderParams {
                    base_color: Color::TRANSPARENT,
                    width: self.width,
                    height: self.height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .expect("come onnnn");

        surface_texture.present();
    }
}

impl SeatHandler for Vencoord {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard_state.is_none() {
            self.keyboard_state = Some(
                self.seat_state
                    .get_keyboard(qh, &seat, None)
                    .expect("how??"),
            );
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Vencoord {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {
        match event.keysym {
            Keysym::Escape => self.exit = true,
            _ => {}
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wayland_client::protocol::wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: smithay_client_toolkit::seat::keyboard::Modifiers,
    ) {
    }
}

delegate_compositor!(Vencoord);
delegate_output!(Vencoord);

delegate_seat!(Vencoord);

delegate_layer!(Vencoord);

delegate_keyboard!(Vencoord);

delegate_registry!(Vencoord);

impl ProvidesRegistryState for Vencoord {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
