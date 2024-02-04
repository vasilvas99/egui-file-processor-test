#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use eframe::egui;
use rayon::prelude::*;
use std::collections::HashSet;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([450.0, 400.0]) // Increased height for more space
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Drag and drop file processor",
        options,
        Box::new(|_cc| {
            Box::from(MyApp {
                dropped_files: HashSet::new(),
                file_processing_thread: FileProcessingThread::new(),
                processing_btn_enabled: true,
                result_msg: String::new(),
            })
        }),
    )
}

struct MyApp {
    dropped_files: HashSet<PathBuf>,
    file_processing_thread: FileProcessingThread,
    processing_btn_enabled: bool,
    result_msg: String,
}

#[derive(Clone, Copy, PartialEq)]
enum ThreadState {
    Uninitialized,
    Initialized,
    Running,
    Done,
}

struct FileProcessingThread {
    state: Arc<Mutex<ThreadState>>,
    files_to_process: Arc<Mutex<Vec<PathBuf>>>,
    processing_results: Arc<Mutex<Vec<Result<()>>>>,
}

impl FileProcessingThread {
    pub fn new() -> Self {
        FileProcessingThread {
            state: Arc::from(Mutex::new(ThreadState::Uninitialized)),
            files_to_process: Arc::from(Mutex::new(vec![])),
            processing_results: Arc::from(Mutex::new(vec![])),
        }
    }

    pub fn set_file_list(&self, file_list: Vec<PathBuf>) {
        *self.files_to_process.lock().unwrap() = file_list;
        *self.state.lock().unwrap() = ThreadState::Initialized;
    }

    fn process_file(file: PathBuf) -> Result<()> {
        thread::sleep(Duration::from_secs(1));
        Err(anyhow::anyhow!(
            "Slept thread for 1 second for file {:?}",
            file
        ))
    }

    pub fn is_in_state(&self, state: ThreadState) -> bool {
        self.get_state() == state
    }

    pub fn run(&self) {
        assert!(
            self.is_in_state(ThreadState::Initialized),
            "Uninitialized file list, use set_file_list()"
        );

        *self.state.clone().lock().unwrap() = ThreadState::Running;

        let files_to_process_ref = self.files_to_process.clone();
        let processing_results_ref = self.processing_results.clone();
        let thread_state_ref = self.state.clone();
        thread::spawn(move || {
            let processing_results: Vec<_> = files_to_process_ref
                .lock()
                .unwrap()
                .par_iter()
                .map(|p| Self::process_file(p.clone()))
                .collect();
            *processing_results_ref.lock().unwrap() = processing_results;
            *thread_state_ref.lock().unwrap() = ThreadState::Done;
        });
    }

    pub fn get_state(&self) -> ThreadState {
        *self.state.as_ref().lock().unwrap()
    }

    pub fn get_results(&self) -> Vec<Result<()>> {
        let r = &*self.processing_results.lock().unwrap();
        r.iter()
            .map(|res| match res {
                Ok(_) => Ok(()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            })
            .collect()
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Drag-and-drop files onto the window");

            let central_panel_rect = ui.available_rect_before_wrap();

            if self.dropped_files.is_empty() {
                return;
            }

            egui::containers::ScrollArea::vertical()
                .max_height(central_panel_rect.height() / 2.0)
                .max_width(central_panel_rect.width())
                .show(ui, |ui| {
                    self.draw_files_list(ui);
                });

            ui.add_enabled_ui(self.processing_btn_enabled, |ui: &mut egui::Ui| {
                let prcocess_btn = ui.button("Process");
                if prcocess_btn.clicked() {
                    self.start_processing_files();
                };
            });

            if !self.processing_btn_enabled {
                ui.spinner();
            }

            egui::containers::ScrollArea::vertical()
                .max_height(central_panel_rect.height() / 2.0)
                .max_width(central_panel_rect.width())
                .id_source("output scroll area")
                .show(ui, |ui| {
                    ui.text_edit_multiline(&mut self.result_msg);
                });

            if self.file_processing_thread.is_in_state(ThreadState::Done) {
                self.gather_processing_results();
            }
        });

        // Collect dropped files:
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                let file_paths: Vec<_> = i
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|p| p.path.clone())
                    .collect();
                self.dropped_files.extend(file_paths);
            }

            if self.dropped_files.is_empty() {
                self.result_msg = String::new();
            }
        });
    }
}

impl MyApp {
    fn draw_files_list(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            let mut files_to_retain = vec![true; self.dropped_files.len()];
            ui.vertical(|ui| {
                for (index, file) in self.dropped_files.iter().enumerate() {
                    let display_label: String = file.display().to_string();

                    ui.horizontal(|ui| {
                        if ui.button("❌").clicked() {
                            files_to_retain[index] = false;
                        }

                        ui.label(display_label);
                    });
                }
            });

            // Retain files based on removal button clicks:
            let mut iter = files_to_retain.iter();
            self.dropped_files.retain(|_| *iter.next().unwrap());
        });
    }

    fn gather_processing_results(&mut self) {
        // gather results, cleanup thread
        let results = self.file_processing_thread.get_results();
        let mut errors = vec![];
        let _ = results
            .into_iter()
            .filter_map(|r| r.map_err(|e| errors.push(e)).ok())
            .collect::<Vec<_>>();
        let err_msgs = errors.iter().map(|e| format!("{e}")).collect::<Vec<_>>();

        if err_msgs.is_empty() {
            self.result_msg = String::from("Success!");
        } else {
            self.result_msg = err_msgs.join("\n")
        }

        self.file_processing_thread = FileProcessingThread::new();
        self.processing_btn_enabled = true;
    }

    fn start_processing_files(&mut self) {
        let files_as_list = self.dropped_files.clone().into_iter().collect();
        self.file_processing_thread.set_file_list(files_as_list);
        self.file_processing_thread.run();

        self.processing_btn_enabled = false;
        self.result_msg = String::new();
    }
}
