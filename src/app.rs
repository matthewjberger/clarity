use crate::world::World;

#[cfg(feature = "wasm32")]
use crate::gltf::import_gltf_slice;

pub struct Viewer {
    angle: f32,
    scene: std::sync::Arc<egui::mutex::Mutex<crate::scene::Scene>>,
    _world: World,

    #[cfg(target_arch = "wasm32")]
    file_receiver: Option<futures::channel::oneshot::Receiver<Vec<u8>>>,
}

impl Viewer {
    /// Called once before the first frame.
    pub fn new<'a>(cc: &'a eframe::CreationContext<'a>) -> Self {
        let gl = cc.gl.as_ref().expect("Failed to get a GL context");
        Self {
            angle: 0.0,
            scene: std::sync::Arc::new(egui::mutex::Mutex::new(
                crate::scene::Scene::new(gl).expect("Failed to create triangle"),
            )),
            _world: World::default(),

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
        let scene = self.scene.clone();

        let cb = eframe::egui_glow::CallbackFn::new(move |_info, painter| {
            scene.lock().paint(painter.gl(), angle);
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
                    let world = crate::gltf::import_gltf_slice(&bytes);
                    log::info!(
                        "Found {} meshes and {} nodes",
                        world.meshes.len(),
                        world.nodes.len()
                    );
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
                    let _world = crate::gltf::import_gltf_file(path);
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
            self.scene.lock().destroy(gl);
        }
    }
}
