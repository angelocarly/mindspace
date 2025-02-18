use std::ops::Mul;
use std::sync::{Arc, Mutex};
use cen::app::App;
use cen::app::app::AppConfig;
use cen::app::gui::GuiComponent;
use cen::graphics::renderer::RenderComponent;
use dotenv::dotenv;
use egui::{Align2, Checkbox, Slider, TextWrapMode, Vec2};
use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};
use ordered_float::OrderedFloat;
use graph::Graph;
use crate::database::{Concept, Database, Relation};
use crate::renderer::{GraphRenderer, RenderNode};

mod database;
mod graph;
mod renderer;

struct Application {
    database: Database,
    graph_renderer: Arc<Mutex<GraphRenderer>>,
    graph: Arc<Mutex<Graph>>,
    concepts: Vec<Concept>,
    relations: Vec<Relation>,
    messages: Vec<String>,
    view_transform: Mat4,
    screen_transform_ortho: Mat4,
    screen_transform_pers: Mat4,
    perspective_camera: bool,
    selected_nodes: Vec<usize>,
}

impl Application {

    async fn reload_graph(&mut self) {

        // tokio::spawn(async {
            self.concepts = self.database.get_concepts().await;
            self.messages = self.database.get_messages().await;
            self.relations = self.database.get_relations().await;
            let mut lock = self.graph.lock().unwrap();
            lock.reset();

            for _ in 0..self.concepts.len() {
                lock.add_node();
            }
            for _ in 0..self.messages.len() {
                lock.add_node();
            }
            for r in &self.relations {
                lock.add_edge(r.a, r.b);
            }
        // });
    }

    async fn new(graph_renderer: Arc<Mutex<GraphRenderer>>) -> Application {

        let database = Database::new().await;
        let concepts = database.get_concepts().await;
        let messages = database.get_messages().await;
        let relations = database.get_relations().await;

        let mut graph = Graph::new();
        for _ in 0..concepts.len() {
            graph.add_node();
        }
        for _ in 0..messages.len() {
            graph.add_node();
        }
        for r in &relations {
            graph.add_edge(r.a, r.b);
        }
        graph.randomize();

        // Transform
        let width = 1600.;
        let height = 900.;
        let aspect_ratio = width / height;

        // ortho
        let scale = Mat4::from_scale(Vec3::new(1. ,aspect_ratio, 1.));
        let projection_ortho = Mat4::orthographic_rh_gl(0., width * 2., 0., height * 2., -1., 1.).inverse();
        let screen_transform_ortho = projection_ortho * scale;

        // pers
        let translate = Mat4::from_translation(Vec3::new(0., 0., 1.2));
        let screen_translate = Mat4::from_translation(Vec3::new(width, height, 1.));
        let scale_pers = Mat4::from_scale(Vec3::new(width, height, 1.));
        let projection = Mat4::perspective_rh(1.0, aspect_ratio, 0.01, 10.);
        let screen_transform_pers = screen_translate * scale_pers * projection * translate;

        let mut p1 = screen_transform_pers * Vec4::new(1., 1., 0., 1.);
        p1 = p1 / p1.w;
        println!("p1 {}", p1);

        let mut p0 = screen_transform_pers * Vec4::new(0., 0., 0., 1.);
        p0 = p0 / p0.w;
        println!("p0 {}", p0);

        let mut pm1 = screen_transform_pers * Vec4::new(-1., -1., 0., 1.);
        pm1 = pm1 / pm1.w;
        println!("pm1 {}", pm1);

        Self {
            database,
            graph: Arc::new(Mutex::new(graph)),
            concepts,
            relations,
            messages,
            graph_renderer: graph_renderer.clone(),
            screen_transform_ortho,
            screen_transform_pers,
            view_transform: Mat4::from_scale(Vec3::new(1., 1., 1.)),
            perspective_camera: true,
            selected_nodes: vec![],
        }
    }
}

impl GuiComponent for Application {
    fn gui(&mut self, context: &egui::Context) {

        // Todo: Move to an update call
        // Update graph data
        let mut lock = self.graph.lock().unwrap();
        lock.update();


        // Gui code
        context.input(|x| {

            if x.pointer.button_down(egui::PointerButton::Primary) {
                let rot_x = glam::Mat4::from_rotation_y(x.pointer.delta().x * 0.5 / 60.0);
                let rot_y = glam::Mat4::from_rotation_x(-x.pointer.delta().y * 0.5 / 60.0);
                self.view_transform = rot_x * rot_y * self.view_transform;
            }

            if x.key_pressed(egui::Key::Backspace) {
                self.selected_nodes.sort();
                for n in self.selected_nodes.iter().rev() {
                    lock.delete_node(*n);
                }
                self.selected_nodes.clear();
            }

            if x.pointer.button_pressed(egui::PointerButton::Primary) {

                if let Some(mut p) = x.pointer.press_origin() {
                    p = p * 2.;

                    let mat = self.screen_transform_pers * self.view_transform;
                    let mut wp = mat.inverse() * Vec4::new(p.x, p.y, 0., 1.);
                    wp = wp / wp.w;
                    let mut wp_f = mat.inverse() * Vec4::new(p.x, p.y, 10., 1.);
                    wp_f = wp_f / wp_f.w;
                    let mut dir = (wp_f - wp).xyz().normalize();
                    // println!("wp {:?}", wp);
                    // println!("wpf {:?}", wp_f);
                    // println!("dir {:?}", dir);

                    // Do ray marching
                    let mut t = 0.0;
                    for _ in 0..100 {
                        let rp = wp.xyz() + dir * t;
                        let near = lock.get_nodes_mut().iter().enumerate()
                            .min_by_key(|&(i, n)| {
                                OrderedFloat((n.pos - rp).length() - 0.01)
                            })
                            .map(|(i, n)| {
                                (i, (n.pos - rp).length() - 0.01)
                            }).unwrap();

                        t += near.1;

                        if near.1 < 0.0001 {
                            if !x.modifiers.shift {
                                self.selected_nodes.clear();
                            }

                            if self.selected_nodes.contains(&near.0) {
                                let index = self.selected_nodes.iter().find(|n| **n == near.0).unwrap();
                                self.selected_nodes.remove(*index);
                                break;
                            }
                            self.selected_nodes.push(near.0);
                            break;
                        }

                        if near.1 > 1000. {
                            break;
                        }
                    }
                }
            }
        });

        let mut positions = lock.get_positions().iter().map(
            |p| {
                RenderNode {
                    p: *p,
                    v: 0
                }
            }
        ).collect::<Vec<RenderNode>>();
        for n in self.selected_nodes.iter() {
            positions[*n].v = 1;
        }

        let edges = lock.get_edges().iter().map(
            |p| (positions[p.0].p, positions[p.1].p)
        ).collect::<Vec<(Vec3, Vec3)>>();
        self.graph_renderer.lock().unwrap().graph_data(positions, edges);

        // Show selected nodes' details
        for n in self.selected_nodes.iter() {
            let node = &lock.get_nodes_mut()[*n];
            let mut screen_pos = self.screen_transform_pers * self.view_transform * Vec4::new(node.pos.x, node.pos.y, node.pos.z, 1.0);
            screen_pos = screen_pos / screen_pos.w;

            egui::Window::new(format!("Node {}", n))
                .resizable(false)
                .title_bar(false)
                .anchor(Align2::LEFT_BOTTOM, [screen_pos.x / 2., screen_pos.y / 2. - 900.])
                .show(context, |ui| unsafe {
                    ui.label(format!("{}", n));
                });

        }

        // Show details on hover
        if let Some(p) = context.pointer_latest_pos() {
            for (id, node) in lock.get_nodes_mut().iter().enumerate() {
                let mut screen_pos = self.screen_transform_pers * self.view_transform * Vec4::new(node.pos.x, node.pos.y, node.pos.z, 1.0);
                screen_pos = screen_pos / screen_pos.w;

                let dist = (Vec2::new(screen_pos.x, screen_pos.y) - Vec2::new(p.x, p.y) * 2.).length();
                if dist < 15. {

                    if self.selected_nodes.contains(&id) {
                        // Already selected nodes already have their details shown
                        continue;
                    }

                    egui::Window::new(format!("Node {}", id))
                        .title_bar(false)
                        .resizable(false)
                        .anchor(Align2::LEFT_BOTTOM, [screen_pos.x / 2., screen_pos.y / 2. - 900.])
                        .show(context, |ui| unsafe {
                            ui.label(format!("{}", id));
                        });

                    break;
                }
            }
        }

        egui::Window::new("Nodes")
            .resizable(true)
            .title_bar(true)
            .show(context, |ui| unsafe {
                ui.label("Edge attraction");
                ui.add(
                    Slider::new(lock.get_edge_strength(), 0.0..=300.0)
                );
                ui.label("Repulsion");
                ui.add(
                    Slider::new(lock.get_repulsion(), 0.0..=1.0)
                );
                ui.label("Center attraction");
                ui.add(
                    Slider::new(lock.get_center_attraction_mut(), 0.0..=300.0)
                );

                ui.add(Checkbox::new(&mut self.perspective_camera, "Use perspective camera"));

                if self.perspective_camera {
                    self.graph_renderer.lock().unwrap().transform(self.screen_transform_pers * self.view_transform);
                } else {
                    self.graph_renderer.lock().unwrap().transform(self.screen_transform_ortho * self.view_transform);
                }

                if ui.button("Randomize").clicked() {
                    lock.randomize();
                }
            });

        egui::Window::new("Messages")
            .resizable(true)
            .title_bar(true)
            .show(context, |ui| {
                use egui_extras::{Column, TableBuilder};

                ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);

                let available_height = ui.available_height();
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto())
                    .column(
                        Column::remainder()
                            .at_least(40.0)
                            .clip(false)
                            .resizable(true),
                    )
                    .min_scrolled_height(0.0)
                    .max_scroll_height(available_height);

                table
                    .header(15.0, move |mut header| {
                        header.col(|ui| {
                            egui::Sides::new().show(
                                ui,
                                |ui| {
                                    ui.strong("Row");
                                },
                                |ui| {
                                },
                            );
                        });
                        header.col(|ui| {
                            ui.strong("Message");
                        });
                    })
                    .body(|mut body| {
                        for row_index in 0..self.messages.len() {
                            body.row(30.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(row_index.to_string());
                                });
                                row.col(|ui| {
                                    ui.label(self.messages[row_index].to_string());
                                });
                            });
                        }
                    });
            }
            );

        egui::Window::new("Concepts")
            .resizable(true)
            .title_bar(true)
            .show(context, |ui| {
                use egui_extras::{Column, TableBuilder};

                let available_height = ui.available_height();
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto())
                    .column(
                        Column::remainder()
                            .at_least(40.0)
                            .clip(true)
                            .resizable(true),
                    )
                    .min_scrolled_height(0.0)
                    .max_scroll_height(available_height);

                table
                    .header(15.0, move |mut header| {
                        header.col(|ui| {
                            egui::Sides::new().show(
                                ui,
                                |ui| {
                                    ui.strong("Row");
                                },
                                |ui| {
                                },
                            );
                        });
                        header.col(|ui| {
                            ui.strong("Concept");
                        });
                    })
                    .body(|mut body| {
                        for row_index in 0..self.concepts.len() {
                            body.row(30.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(row_index.to_string());
                                });
                                row.col(|ui| {
                                    ui.label(self.concepts[row_index].name.to_string());
                                });
                            });
                        }
                });
            }
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize .env environment variables
    dotenv().ok();

    let renderer = Arc::new(Mutex::new(GraphRenderer::new()));
    let application = Arc::new(Mutex::new(Application::new(renderer.clone()).await));
    App::run(
        AppConfig::default()
            .width(1600)
            .height(900)
            .log_fps(false)
            .vsync(true),
        renderer,
        Some(application.clone())
    );

    Ok(())
}
