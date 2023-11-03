use glow::*;

fn main() {
    unsafe {
        // Create a context from a glutin window on non-wasm32 targets
        #[cfg(feature = "glutin")]
        let (shader_version, mut window, event_loop, gl_config) = {
            use glutin::prelude::GlConfig;

            let event_loop = winit::event_loop::EventLoop::new().unwrap();
            let window_builder = winit::window::WindowBuilder::new()
                .with_title("Hello triangle!")
                .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0));

            let template = glutin::config::ConfigTemplateBuilder::new().with_alpha_size(8);
            let display_builder =
                glutin_winit::DisplayBuilder::new().with_window_builder(Some(window_builder));

            let (window, gl_config) = display_builder
                .build(&event_loop, template, |configs| {
                    // Find the config with the maximum number of samples, so our triangle will
                    // be smooth.
                    configs
                        .reduce(|accum, config| {
                            let transparency_check =
                                config.supports_transparency().unwrap_or(false)
                                    & !accum.supports_transparency().unwrap_or(false);

                            if transparency_check || config.num_samples() > accum.num_samples() {
                                config
                            } else {
                                accum
                            }
                        })
                        .unwrap()
                })
                .unwrap();

            ("#version 410", window, event_loop, gl_config)
        };

        // We handle events differently between targets
        #[cfg(feature = "glutin")]
        {
            use glutin::display::GetGlDisplay;
            use glutin::prelude::GlDisplay;
            use glutin::prelude::NotCurrentGlContext;
            use glutin::surface::GlSurface;
            use glutin_winit::GlWindow;
            use raw_window_handle::HasRawWindowHandle;
            use winit::event::{Event, WindowEvent};

            let gl_display = gl_config.display();

            let raw_window_handle = window.as_ref().map(|window| window.raw_window_handle());

            let context_attributes =
                glutin::context::ContextAttributesBuilder::new().build(raw_window_handle);

            let mut not_current_gl_context = Some({
                gl_display
                    .create_context(&gl_config, &context_attributes)
                    .unwrap_or_else(|_| {
                        // gl_display.create_context(&gl_config, &fallback_context_attributes).unwrap_or_else(
                        //     |_| {
                        //         gl_display
                        //             .create_context(&gl_config, &legacy_context_attributes)
                        //             .expect("failed to create context")
                        //     },
                        // )
                        todo!("fallback context stuff");
                    })
            });

            let mut state = None;

            event_loop
                .run(move |event, window_target| {
                    match event {
                        Event::Resumed => {
                            let window = window.take().unwrap_or_else(|| {
                                let window_builder =
                                    winit::window::WindowBuilder::new().with_transparent(true);
                                glutin_winit::finalize_window(
                                    window_target,
                                    window_builder,
                                    &gl_config,
                                )
                                .unwrap()
                            });

                            let attrs = window.build_surface_attributes(<_>::default());
                            let gl_surface = {
                                glutin::prelude::GlDisplay::create_window_surface(
                                    &glutin::display::GetGlDisplay::display(&gl_config),
                                    &gl_config,
                                    &attrs,
                                )
                                .unwrap()
                            };

                            // Make it current.
                            let gl_context = not_current_gl_context
                                .take()
                                .unwrap()
                                .make_current(&gl_surface)
                                .unwrap();

                            // Try setting vsync.
                            if let Err(res) = gl_surface.set_swap_interval(
                                &gl_context,
                                glutin::surface::SwapInterval::Wait(
                                    std::num::NonZeroU32::new(1).unwrap(),
                                ),
                            ) {
                                eprintln!("Error setting vsync: {res:?}");
                            }

                            let gl = glow::Context::from_loader_function(|s| {
                                gl_config
                                    .display()
                                    .get_proc_address(&std::ffi::CString::new(s).unwrap())
                                    as *const _
                            });

                            let vertex_array = gl
                                .create_vertex_array()
                                .expect("Cannot create vertex array");
                            gl.bind_vertex_array(Some(vertex_array));

                            let program = gl.create_program().expect("Cannot create program");

                            let (vertex_shader_source, fragment_shader_source) = (
                                r#"const vec2 verts[3] = vec2[3](
                                vec2(0.5f, 1.0f),
                                vec2(0.0f, 0.0f),
                                vec2(1.0f, 0.0f)
                            );
                            out vec2 vert;
                            void main() {
                                vert = verts[gl_VertexID];
                                gl_Position = vec4(vert - 0.5, 0.0, 1.0);
                            }"#,
                                r#"precision mediump float;
                            in vec2 vert;
                            out vec4 color;
                            void main() {
                                color = vec4(vert, 0.5, 1.0);
                            }"#,
                            );

                            let shader_sources = [
                                (glow::VERTEX_SHADER, vertex_shader_source),
                                (glow::FRAGMENT_SHADER, fragment_shader_source),
                            ];

                            let mut shaders = Vec::with_capacity(shader_sources.len());

                            for (shader_type, shader_source) in shader_sources.iter() {
                                let shader = gl
                                    .create_shader(*shader_type)
                                    .expect("Cannot create shader");
                                gl.shader_source(
                                    shader,
                                    &format!("{}\n{}", shader_version, shader_source),
                                );
                                gl.compile_shader(shader);
                                if !gl.get_shader_compile_status(shader) {
                                    panic!("{}", gl.get_shader_info_log(shader));
                                }
                                gl.attach_shader(program, shader);
                                shaders.push(shader);
                            }

                            gl.link_program(program);
                            if !gl.get_program_link_status(program) {
                                panic!("{}", gl.get_program_info_log(program));
                            }

                            for shader in shaders {
                                gl.detach_shader(program, shader);
                                gl.delete_shader(shader);
                            }

                            gl.use_program(Some(program));
                            gl.clear_color(0.1, 0.2, 0.3, 1.0);

                            assert!(state
                                .replace((gl_context, gl_surface, window, gl))
                                .is_none());
                        }
                        Event::WindowEvent { event, .. } => match event {
                            WindowEvent::Resized(size) => {
                                if size.width != 0 && size.height != 0 {
                                    // Some platforms like EGL require resizing GL surface to update the size
                                    // Notable platforms here are Wayland and macOS, other don't require it
                                    // and the function is no-op, but it's wise to resize it for portability
                                    // reasons.
                                    if let Some((gl_context, gl_surface, _, _)) = &state {
                                        gl_surface.resize(
                                            gl_context,
                                            std::num::NonZeroU32::new(size.width).unwrap(),
                                            std::num::NonZeroU32::new(size.height).unwrap(),
                                        );
                                    }
                                }
                            }
                            WindowEvent::CloseRequested => window_target.exit(),
                            _ => (),
                        },
                        Event::AboutToWait => {
                            if let Some((gl_context, gl_surface, window, gl)) = &state {
                                gl.clear(glow::COLOR_BUFFER_BIT);
                                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                                window.request_redraw();

                                gl_surface.swap_buffers(gl_context).unwrap();
                            }
                        }
                        _ => (),
                    }
                })
                .unwrap();
        }
    }
}
