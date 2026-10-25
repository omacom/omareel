//! Qt behaviour that omareel's files and output depend on, reproduced exactly. Each module
//! names the Qt API it stands in for; tests compare against goldens recorded from Qt itself.

pub mod cpp;
pub mod geometry;
pub mod qchar;
pub mod qcolor;
pub mod qdir;
pub mod qenv;
pub mod qfile;
pub mod qjson;
pub mod qnumber;
pub mod qprocess;
pub mod qround;
pub mod qsavefile;
pub mod qstandardpaths;
pub mod qstring;
pub mod qtemporaryfile;
pub mod qurl;
