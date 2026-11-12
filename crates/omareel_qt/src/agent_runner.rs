//! The editor's `agent` object: Polish, "Ask the editor…" and "Write description" run the
//! person's coding agent on the open bundle (omareel_core::agent::runner), and the chapters it
//! writes (chapters.json) come back for the timeline's ruler and the YouTube description.
//!
//! The agent edits through `omareel apply`, so its edits reach the editor through the live
//! reload of project.json, each as a step the person can undo; this object only starts and
//! stops the agent and reports what it does, as `entry` signals the drawer lists.

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use omareel_core::agent::runner::{Choice, Event, Runner, Task, TaskKind};
use omareel_core::chapters::Chapters;
use omareel_core::compat::{qdir, qjson};
use omareel_core::project::Project;
use std::thread::JoinHandle;
use std::time::SystemTime;

use crate::export::{json_map, map};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    #[namespace = "Omareel"]
    extern "RustQt" {
        #[qobject]
        // The default agent: its name, where it sends what it reads, and why it can't run.
        #[qproperty(QString, agent_name, cxx_name = "agentName", READ, NOTIFY = agent_changed)]
        #[qproperty(QString, provider, READ, NOTIFY = agent_changed)]
        #[qproperty(bool, available, READ, NOTIFY = agent_changed)]
        #[qproperty(QString, problem, READ, NOTIFY = agent_changed)]
        #[qproperty(bool, running, READ, NOTIFY = running_changed)]
        // "polish", "ask", "chapters" while running; the last one after.
        #[qproperty(QString, task, READ, NOTIFY = running_changed)]
        // chapters.json: {exists, title, description, chapters: [{source, title}]}.
        #[qproperty(QMap_QString_QVariant, chapters, READ, NOTIFY = chapters_changed)]
        type AgentRunner = super::AgentRunnerRust;

        #[qsignal]
        #[cxx_name = "agentChanged"]
        fn agent_changed(self: Pin<&mut AgentRunner>);
        #[qsignal]
        #[cxx_name = "runningChanged"]
        fn running_changed(self: Pin<&mut AgentRunner>);
        #[qsignal]
        #[cxx_name = "chaptersChanged"]
        fn chapters_changed(self: Pin<&mut AgentRunner>);
        /// Something the agent did: `kind` is started, message, command, apply, image,
        /// problem, finished or stopped; `detail` the event as `omareel agent` prints it.
        #[qsignal]
        fn entry(self: Pin<&mut AgentRunner>, kind: QString, text: QString, detail: QMap_QString_QVariant);
        /// The run ended.
        #[qsignal]
        fn finished(self: Pin<&mut AgentRunner>, ok: bool, message: QString);

        #[qinvokable]
        fn polish(self: Pin<&mut AgentRunner>);
        #[qinvokable]
        fn ask(self: Pin<&mut AgentRunner>, request: &QString);
        #[qinvokable]
        #[cxx_name = "writeDescription"]
        fn write_description(self: Pin<&mut AgentRunner>);
        #[qinvokable]
        fn stop(self: Pin<&mut AgentRunner>);
        /// Looks for the default agent again (it may have changed in Omarchy).
        #[qinvokable]
        #[cxx_name = "refreshAgent"]
        fn refresh_agent(self: Pin<&mut AgentRunner>);
        /// Reads chapters.json again when it changed.
        #[qinvokable]
        #[cxx_name = "refreshChapters"]
        fn refresh_chapters(self: Pin<&mut AgentRunner>);
        /// The chapters on the edited timeline, `[{time, title, source}]`, for the clips
        /// `JSON.stringify(editor.clips)`.
        #[qinvokable]
        #[cxx_name = "chapterMarkers"]
        fn chapter_markers(self: &AgentRunner, clips_json: &QString) -> QList_QVariant;
        /// The title, description and `00:00 Chapter` lines for YouTube.
        #[qinvokable]
        #[cxx_name = "youtubeDescription"]
        fn youtube_description(self: &AgentRunner, clips_json: &QString, duration: f64) -> QString;
    }

    impl cxx_qt::Threading for AgentRunner {}
    impl cxx_qt::Constructor<(QString,), NewArguments = (QString,)> for AgentRunner {}
}

pub struct AgentRunnerRust {
    bundle: String,
    choice: Choice,
    agent_name: QString,
    provider: QString,
    available: bool,
    problem: QString,
    running: bool,
    task: QString,
    chapters: QMap<QMapPair_QString_QVariant>,
    chapter_data: Option<Chapters>,
    chapters_stamp: Option<SystemTime>,
    runner: Runner,
    worker: Option<JoinHandle<()>>,
}

impl Drop for AgentRunnerRust {
    /// The window is closing: the agent stops with it, rather than editing on unseen.
    fn drop(&mut self) {
        self.runner.stop();
    }
}

impl cxx_qt::Constructor<(QString,)> for qobject::AgentRunner {
    type BaseArguments = ();
    type InitializeArguments = ();
    type NewArguments = (QString,);

    fn route_arguments(arguments: (QString,)) -> (Self::NewArguments, Self::BaseArguments, Self::InitializeArguments) {
        (arguments, (), ())
    }

    fn new((bundle,): (QString,)) -> AgentRunnerRust {
        AgentRunnerRust {
            bundle: qdir::absolute_path(&bundle.to_string()),
            choice: Choice::NoneChosen,
            agent_name: QString::default(),
            provider: QString::default(),
            available: false,
            problem: QString::default(),
            running: false,
            task: QString::default(),
            chapters: QMap::default(),
            chapter_data: None,
            chapters_stamp: None,
            runner: Runner::new(),
            worker: None,
        }
    }

    fn initialize(mut self: Pin<&mut Self>, _: ()) {
        self.as_mut().refresh_agent();
        self.refresh_chapters();
    }
}

fn clips_from(json: &QString) -> Vec<omareel_core::project::Clip> {
    let root = qjson::object_from_json(format!("{{\"clips\": {json}}}").as_bytes()).unwrap_or_default();
    Project::from_json(&root).clips
}

impl qobject::AgentRunner {
    fn refresh_agent(mut self: Pin<&mut Self>) {
        let choice = Choice::find();
        let (name, provider) = match &choice {
            Choice::Ready { adapter, .. } | Choice::NotInstalled { adapter } => (adapter.name.to_owned(), adapter.provider),
            Choice::Unsupported { id } => (omareel_core::agent::runner::agent_name(id), ""),
            Choice::NoneChosen => (String::new(), ""),
        };
        let mut this = self.as_mut().rust_mut();
        this.agent_name = QString::from(name.as_str());
        this.provider = QString::from(provider);
        this.available = matches!(choice, Choice::Ready { .. });
        this.problem = QString::from(choice.problem().as_str());
        this.choice = choice;
        self.agent_changed();
    }

    fn refresh_chapters(mut self: Pin<&mut Self>) {
        let path = omareel_core::chapters::path(&self.bundle);
        let stamp = std::fs::metadata(&path).and_then(|metadata| metadata.modified()).ok();
        if stamp == self.chapters_stamp && (stamp.is_some() || self.chapter_data.is_none()) {
            return;
        }
        // A file being written reads as nothing yet; the next look takes it in.
        let loaded = Chapters::load(&self.bundle).ok().flatten();
        let mut object = loaded.as_ref().map(Chapters::to_json).unwrap_or_default();
        object.insert("exists", loaded.is_some());
        let mut this = self.as_mut().rust_mut();
        this.chapters_stamp = stamp;
        this.chapter_data = loaded;
        this.chapters = json_map(&object);
        self.chapters_changed();
    }

    fn chapter_markers(&self, clips_json: &QString) -> QList<QVariant> {
        let Some(chapters) = &self.chapter_data else { return QList::default() };
        chapters
            .on_timeline(&clips_from(clips_json))
            .into_iter()
            .map(|chapter| {
                QVariant::from(&map([
                    ("time", QVariant::from(&chapter.output)),
                    ("source", QVariant::from(&chapter.source)),
                    ("title", QVariant::from(&QString::from(chapter.title.as_str()))),
                ]))
            })
            .collect()
    }

    fn youtube_description(&self, clips_json: &QString, duration: f64) -> QString {
        let Some(chapters) = &self.chapter_data else { return QString::default() };
        QString::from(chapters.youtube_description(&clips_from(clips_json), duration).as_str())
    }

    fn polish(self: Pin<&mut Self>) {
        self.start(TaskKind::Polish);
    }

    fn ask(self: Pin<&mut Self>, request: &QString) {
        let request = request.to_string();
        if !request.trim().is_empty() {
            self.start(TaskKind::Ask(request));
        }
    }

    fn write_description(self: Pin<&mut Self>) {
        self.start(TaskKind::Chapters);
    }

    fn stop(self: Pin<&mut Self>) {
        if self.running {
            self.runner.stop();
        }
    }

    fn start(mut self: Pin<&mut Self>, kind: TaskKind) {
        if self.running {
            return;
        }
        self.as_mut().refresh_agent();
        let Choice::Ready { adapter, program } = self.choice.clone() else {
            let problem = self.problem.clone();
            self.as_mut().entry(QString::from("problem"), problem.clone(), QMap::default());
            self.finished(false, problem);
            return;
        };
        if let Some(worker) = self.as_mut().rust_mut().worker.take() {
            let _ = worker.join();
        }
        let task = match Task::new(&self.bundle, kind) {
            Ok(task) => task,
            Err(error) => {
                let error = QString::from(error.as_str());
                self.as_mut().entry(QString::from("problem"), error.clone(), QMap::default());
                self.finished(false, error);
                return;
            }
        };
        {
            let mut this = self.as_mut().rust_mut();
            this.runner.reset();
            this.running = true;
            this.task = QString::from(task.kind.name());
        }
        self.as_mut().running_changed();
        let runner = self.runner.clone();
        let thread = self.qt_thread();
        let worker = std::thread::spawn(move || {
            let events = thread.clone();
            let result = runner.run(adapter, &program, &task, move |event| {
                let _ = events.queue(move |runner| runner.deliver(event));
            });
            let _ = thread.queue(move |runner| runner.done(result));
        });
        self.as_mut().rust_mut().worker = Some(worker);
    }

    fn deliver(mut self: Pin<&mut Self>, event: Event) {
        if matches!(event, Event::Applied { .. } | Event::Finished { .. }) {
            self.as_mut().refresh_chapters();
        }
        let detail = json_map(&event.to_json());
        self.entry(QString::from(event.kind()), QString::from(event.text().as_str()), detail);
    }

    fn done(mut self: Pin<&mut Self>, result: Result<omareel_core::agent::runner::Outcome, String>) {
        self.as_mut().rust_mut().running = false;
        self.as_mut().running_changed();
        self.as_mut().refresh_chapters();
        match result {
            Ok(outcome) => self.finished(outcome.ok, QString::from(outcome.summary.as_str())),
            Err(error) => {
                let kind = if self.runner.stopped() { "stopped" } else { "problem" };
                self.as_mut().entry(QString::from(kind), QString::from(error.as_str()), QMap::default());
                self.finished(false, QString::from(error.as_str()));
            }
        }
    }
}
