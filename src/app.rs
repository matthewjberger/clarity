pub struct Viewer {
    angle: f32,
    rotating_triangle: std::sync::Arc<egui::mutex::Mutex<RotatingTriangle>>,

    #[cfg(target_arch = "wasm32")]
    file_receiver: Option<futures::channel::oneshot::Receiver<Vec<u8>>>,
}

impl Viewer {
    /// Called once before the first frame.
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let gl = cc.gl.as_ref().expect("Failed to get a GL context");
        Self {
            angle: 0.0,
            rotating_triangle: std::sync::Arc::new(egui::mutex::Mutex::new(
                RotatingTriangle::new(gl).expect("Failed to create triangle"),
            )),

            #[cfg(target_arch = "wasm32")]
            file_receiver: None,
        }
    }

    fn custom_painting(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(300.0), egui::Sense::drag());

        self.angle += response.drag_delta().x * 0.01;

        // Clone locals so we can move them into the paint callback:
        let angle = self.angle;
        let rotating_triangle = self.rotating_triangle.clone();

        let cb = eframe::egui_glow::CallbackFn::new(move |_info, painter| {
            rotating_triangle.lock().paint(painter.gl(), angle);
        });

        let callback = egui::PaintCallback {
            rect,
            callback: std::sync::Arc::new(cb),
        };
        ui.painter().add(callback);
    }
}

impl eframe::App for Viewer {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(file_receiver) = self.file_receiver.as_mut() {
                if let Ok(Some(bytes)) = file_receiver.try_recv() {
                    log::info!("File received: {} bytes", bytes.len());
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.button("Pick file ...").clicked() {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("GLTF / GLB", &["gltf", "glb"])
                    .pick_file()
                {
                    log::info!("File picked: {path:#?}");
                }

                #[cfg(target_arch = "wasm32")]
                {
                    let (sender, receiver) = futures::channel::oneshot::channel::<Vec<u8>>();
                    self.file_receiver = Some(receiver);
                    let task = rfd::AsyncFileDialog::new()
                        .add_filter("GLTF / GLB", &["gltf", "glb"])
                        .pick_file();
                    wasm_bindgen_futures::spawn_local(async {
                        let file = task.await;
                        if let Some(file) = file {
                            let bytes = file.read().await;
                            let _ = sender.send(bytes);
                        }
                    });
                }
            }

            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                egui::Frame::canvas(ui.style()).show(ui, |ui| {
                    self.custom_painting(ui);
                });
            });
        });
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(gl) = gl {
            self.rotating_triangle.lock().destroy(gl);
        }
    }
}

struct RotatingTriangle {
    program: eframe::glow::Program,
    vertex_array: eframe::glow::VertexArray,
}

#[allow(unsafe_code)] // we need unsafe code to use glow
impl RotatingTriangle {
    fn new(gl: &eframe::glow::Context) -> Option<Self> {
        use eframe::glow::HasContext as _;

        let shader_version = eframe::egui_glow::ShaderVersion::get(gl);

        unsafe {
            let program = gl.create_program().expect("Cannot create program");

            if !shader_version.is_new_shader_interface() {
                log::warn!(
                    "Custom 3D painting hasn't been ported to {:?}",
                    shader_version
                );
                return None;
            }

            let (vertex_shader_source, fragment_shader_source) = (
                r#"
                    const vec2 verts[3] = vec2[3](
                        vec2(0.0, 1.0),
                        vec2(-1.0, -1.0),
                        vec2(1.0, -1.0)
                    );
                    const vec4 colors[3] = vec4[3](
                        vec4(1.0, 0.0, 0.0, 1.0),
                        vec4(0.0, 1.0, 0.0, 1.0),
                        vec4(0.0, 0.0, 1.0, 1.0)
                    );
                    out vec4 v_color;
                    uniform float u_angle;
                    void main() {
                        v_color = colors[gl_VertexID];
                        gl_Position = vec4(verts[gl_VertexID], 0.0, 1.0);
                        gl_Position.x *= cos(u_angle);
                    }
                "#,
                r#"
                    precision mediump float;
                    in vec4 v_color;
                    out vec4 out_color;
                    void main() {
                        out_color = v_color;
                    }
                "#,
            );

            let shader_sources = [
                (eframe::glow::VERTEX_SHADER, vertex_shader_source),
                (eframe::glow::FRAGMENT_SHADER, fragment_shader_source),
            ];

            let shaders: Vec<_> = shader_sources
                .iter()
                .map(|(shader_type, shader_source)| {
                    let shader = gl
                        .create_shader(*shader_type)
                        .expect("Cannot create shader");
                    gl.shader_source(
                        shader,
                        &format!(
                            "{}\n{}",
                            shader_version.version_declaration(),
                            shader_source
                        ),
                    );
                    gl.compile_shader(shader);
                    assert!(
                        gl.get_shader_compile_status(shader),
                        "Failed to compile custom_3d_glow {shader_type}: {}",
                        gl.get_shader_info_log(shader)
                    );

                    gl.attach_shader(program, shader);
                    shader
                })
                .collect();

            gl.link_program(program);
            assert!(
                gl.get_program_link_status(program),
                "{}",
                gl.get_program_info_log(program)
            );

            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            let vertex_array = gl
                .create_vertex_array()
                .expect("Cannot create vertex array");

            Some(Self {
                program,
                vertex_array,
            })
        }
    }

    fn destroy(&self, gl: &eframe::glow::Context) {
        use eframe::glow::HasContext as _;
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vertex_array);
        }
    }

    fn paint(&self, gl: &eframe::glow::Context, angle: f32) {
        use eframe::glow::HasContext as _;
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_1_f32(
                gl.get_uniform_location(self.program, "u_angle").as_ref(),
                angle,
            );
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.draw_arrays(eframe::glow::TRIANGLES, 0, 3);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct ReceivedFile {
    pub id: String,
    pub bytes: Vec<u8>,
    pub tag: String,
}

#[cfg(target_arch = "wasm32")]
pub type FileSystemId = String;

#[cfg(target_arch = "wasm32")]
pub type FileSystemPath = String;

#[cfg(target_arch = "wasm32")]
pub type FileSystemBytes = Vec<u8>;

#[cfg(target_arch = "wasm32")]
pub type FileSystemTag = String;
