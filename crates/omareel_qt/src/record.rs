//! The recording commands `main.cpp` dispatches to `omareel_record`: `omareel record`, the
//! recorder daemon, `__selfview-ipc` and `__record-bar-ipc`. The launcher and the recording bar
//! reach the recorder's state files through `omareel_record` themselves (launcher.rs,
//! recording_bar.rs).

use cxx_qt_lib::{QList, QString, QStringList};
use omareel_record::cli;
use omareel_record::recorder::daemon;

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h (see
        // export.rs).
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        /// `qWarning().noquote() << message`.
        #[namespace = "Omareel::Shims"]
        fn warning(message: &QString);
    }

    #[namespace = "omareel::record"]
    extern "Rust" {
        /// `omareel record <arguments>`: prints what it did and returns the exit status.
        #[cxx_name = "recordCommand"]
        fn record_command(arguments: &QStringList) -> i32;
        /// `omareel __record-daemon <arguments>`: the recorder daemon; its exit status.
        #[cxx_name = "daemonMain"]
        fn daemon_main(arguments: &QStringList) -> i32;
        /// `omareel __selfview-ipc <arguments>`.
        #[cxx_name = "selfViewIpc"]
        fn self_view_ipc(arguments: &QStringList) -> i32;
        /// `omareel __record-bar-ipc <arguments>`.
        #[cxx_name = "recordBarIpc"]
        fn record_bar_ipc(arguments: &QStringList) -> i32;
    }
}

/// A QString as Rust text, through `toUtf8` (which never fails, unlike a UTF-16 conversion of
/// a string holding a lone surrogate).
fn text(string: &QString) -> String {
    String::from_utf8_lossy(string.to_utf8().as_slice()).into_owned()
}

/// The arguments as Rust strings.
fn strings(arguments: &QStringList) -> Vec<String> {
    QList::<QString>::from(arguments).iter().map(text).collect()
}

fn record_command(arguments: &QStringList) -> i32 {
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    cli::record_command(&strings(arguments), &mut out, &mut err)
}

fn daemon_main(arguments: &QStringList) -> i32 {
    daemon::daemon_main(&strings(arguments), &|message| ffi::warning(&QString::from(message)))
}

fn self_view_ipc(arguments: &QStringList) -> i32 {
    cli::self_view_ipc(&strings(arguments))
}

fn record_bar_ipc(arguments: &QStringList) -> i32 {
    cli::record_bar_ipc(&strings(arguments), &mut std::io::stderr().lock())
}
