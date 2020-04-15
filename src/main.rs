#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use std::fs::File;
use std::io::{Cursor, ErrorKind, Read};
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use rocket::{http, Request, Response, response};
use rocket::http::ContentType;
use rocket::response::Responder;
use thiserror::Error;
use zip::result::ZipError;
use zip::ZipArchive;
use mime::{Mime, FromStrError};

fn main() {
    rocket::ignite().mount("/", routes![getfile]).launch();
}

#[get("/<zipfile>/<path..>")]
fn getfile(zipfile: String, path: PathBuf) -> Result<Response<'static>, GetFileError> {
    let mut arc = ZipArchive::new(File::open(&zipfile)?)?;
    let mut file = arc.by_name(path.to_str().unwrap())?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;
    let mime = mime_guess::from_path(path).first_or( Mime::from_str(&tree_magic::from_u8(&content))?);
    let content_type: ContentType = ContentType::from(convert_mime_vers(mime));
    let cur = Cursor::new(content);
    Ok(Response::build().header(content_type).sized_body(cur).finalize())
}

fn convert_mime_vers(mime: Mime) -> mime02::Mime {
    mime02::Mime::from_str(&mime.to_string()).unwrap()
}

#[derive(Debug, Error)]
enum GetFileError {
    #[error("I/O Error")]
    Io(#[from] io::Error),
    #[error("ZIP Error")]
    Zip(#[from] ZipError),
    #[error("MIME Parse Error")]
    MimeParse(#[from] FromStrError),
    #[error("Content Type Parse Error")]
    ContentTypeParse(String),
}

impl<'r> Responder<'r> for GetFileError {
    fn respond_to(self, _request: &Request) -> response::Result<'r> {
        match self {
            GetFileError::Io(e) | GetFileError::Zip(ZipError::Io(e)) if e.kind() == ErrorKind::NotFound => Err(http::Status::NotFound),
            GetFileError::Zip(ZipError::FileNotFound) => Err(http::Status::NotFound),
            _ => Err(http::Status::InternalServerError),
        }
    }
}