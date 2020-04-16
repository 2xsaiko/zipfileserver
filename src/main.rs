#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use std::fs::File;
use std::io::{Cursor, ErrorKind, Read};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use mime::{FromStrError, Mime};
use rocket::{http, Request, Response, response};
use rocket::http::ContentType;
use rocket::response::{Redirect, Responder};
use thiserror::Error;
use zip::read::ZipFile;
use zip::result::{ZipError, ZipResult};
use zip::ZipArchive;

fn main() {
    rocket::ignite().mount("/", routes![getfile_root, getfile]).launch();
}

#[get("/<zipfile>")]
fn getfile_root(zipfile: String) -> Result<FileRequestResponse, FileRequestError> {
    getfile(zipfile, PathBuf::from("/"))
}

#[get("/<zipfile>/<path..>")]
fn getfile(zipfile: String, path: PathBuf) -> Result<FileRequestResponse, FileRequestError> {
    let mut arc = ZipArchive::new(File::open(&zipfile)?)?;
    let (dir_access, mut file) = try_get_file(&mut arc, &path)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
    let mime = mime_guess::from_path(path).first_or(Mime::from_str(&tree_magic::from_u8(&content))?);
    let content_type: ContentType = ContentType::from(convert_mime_vers(mime));
    let cur = Cursor::new(content);
    let response = Response::build().header(content_type).sized_body(cur).finalize();
    Ok(if !dir_access { FileRequestResponse::ByFile(response) } else { FileRequestResponse::ByDir(response) })
}

fn try_get_file<'a>(arc: &'a mut ZipArchive<File>, path: impl AsRef<Path>) -> Result<(bool, ZipFile<'a>), FileRequestError> {
    fn try_get_file0(arc: &mut ZipArchive<File>, path: impl AsRef<Path>) -> Option<ZipResult<ZipFile>> {
        let path = normalize_path(path).unwrap();
        let path = path.strip_prefix("/").unwrap().to_str().unwrap();
        match arc.by_name(path) {
            Ok(f) => Some(Ok(f)),
            Err(ZipError::FileNotFound) => None,
            x => Some(x),
        }
    }

    let is_dir = path.as_ref().to_str().map(|s| s.ends_with('/')).unwrap_or(false);

    let ptr = arc as *mut ZipArchive<File>;
    if let Some(r) = try_get_file0(unsafe { ptr.as_mut::<'a>() }.unwrap(), &path) {
        if is_dir { Err(FileRequestError::Zip(ZipError::FileNotFound)) } else { Ok((false, r?)) }
    } else {
        let index_files = ["index.html"];
        for &entry in index_files.iter() {
            let path = path.as_ref().join(entry);
            if let Some(r) = try_get_file0(unsafe { ptr.as_mut::<'a>() }.unwrap(), path) {
                return Ok((true, r?));
            }
        }
        Err(FileRequestError::Zip(ZipError::FileNotFound))
    }
}

fn normalize_path(path: impl AsRef<Path>) -> Option<PathBuf> {
    let mut pb = PathBuf::from("/");
    for c in path.as_ref().components() {
        match c {
            Component::Prefix(_) => return None,
            Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => {
                pb.pop();
            }
            Component::Normal(s) => pb.push(s),
        }
    }
    Some(pb)
}

// necessary because hyper uses an outdated version of the mime crate
fn convert_mime_vers(mime: Mime) -> mime02::Mime {
    mime02::Mime::from_str(&mime.to_string()).unwrap()
}

#[derive(Debug, Error)]
enum FileRequestError {
    #[error("I/O Error")]
    Io(#[from] io::Error),
    #[error("ZIP Error")]
    Zip(#[from] ZipError),
    #[error("MIME Parse Error")]
    MimeParse(#[from] FromStrError),
}

enum FileRequestResponse {
    ByFile(Response<'static>),
    ByDir(Response<'static>),
}

impl<'r> Responder<'r> for FileRequestError {
    fn respond_to(self, _request: &Request) -> response::Result<'r> {
        match self {
            FileRequestError::Io(e) | FileRequestError::Zip(ZipError::Io(e)) if e.kind() == ErrorKind::NotFound => Err(http::Status::NotFound),
            FileRequestError::Zip(ZipError::FileNotFound) => Err(http::Status::NotFound),
            _ => Err(http::Status::InternalServerError),
        }
    }
}

impl<'r> Responder<'r> for FileRequestResponse {
    fn respond_to(self, request: &Request) -> response::Result<'r> {
        let is_dir = request.uri().path().ends_with('/');
        match self {
            FileRequestResponse::ByFile(_) if is_dir => Err(http::Status::NotFound),
            FileRequestResponse::ByDir(f)  if is_dir => f.respond_to(request),
            FileRequestResponse::ByFile(f) => f.respond_to(request),
            FileRequestResponse::ByDir(_) => {
                use rocket::http::uri::Origin;

                let mut urlstr = format!("{}/", request.uri().segments().0);
                if let Some(q) = request.uri().query() {
                    urlstr.push('?');
                    urlstr.push_str(q);
                }

                Redirect::permanent(Origin::parse_owned(urlstr).unwrap()).respond_to(request)
            },
        }
    }
}