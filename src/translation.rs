use std::{
    env,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value, json};
use windows_webview::WebView;

type Failure = &'static str;
const CAPTURE_ERROR: Failure = "O Brisa não conseguiu ler o texto da página.";
const WORKER_ERROR: Failure =
    "A tradução local falhou. Verifique a conexão para baixar o modelo e tente novamente.";
const CAPTURE_LIMIT: usize = 160_000;
const RESULT_LIMIT: u64 = 640_000;

#[derive(Deserialize)]
pub struct Text {
    pub language: String,
    pub text: Vec<String>,
}

pub struct Capture {
    pub context: i64,
    pub text: Text,
}

fn value(response: windows_result::Result<String>) -> Result<Value, Failure> {
    let data = response.map_err(|_| CAPTURE_ERROR)?;
    if data.len() > CAPTURE_LIMIT {
        return Err(CAPTURE_ERROR);
    }
    serde_json::from_str(&data).map_err(|_| CAPTURE_ERROR)
}

pub fn capture(web: &WebView) -> windows_result::Result<Receiver<Result<Capture, Failure>>> {
    let (send, receive) = mpsc::channel();
    let source = web.clone();
    web.call_dev_tools_protocol_method("Page.getFrameTree", "{}", move |response| {
        let frame = value(response).ok().and_then(|data| {
            data.pointer("/frameTree/frame/id")?
                .as_str()
                .map(str::to_owned)
        });
        let Some(frame) = frame else {
            let _ = send.send(Err(CAPTURE_ERROR));
            return;
        };
        let params = json!({ "frameId": frame, "worldName": "Brisa Local Translation" });
        let page = source.clone();
        let result = send.clone();
        if source
            .call_dev_tools_protocol_method(
                "Page.createIsolatedWorld",
                &params.to_string(),
                move |response| {
                    let Some(context) = value(response)
                        .ok()
                        .and_then(|data| data["executionContextId"].as_i64())
                    else {
                        let _ = result.send(Err(CAPTURE_ERROR));
                        return;
                    };
                    let output = result.clone();
                    if evaluate(
                        &page,
                        context,
                        include_str!("translate-document.js"),
                        move |response| {
                            let capture = value(response).and_then(|mut data| {
                                let body = data
                                    .pointer_mut("/result/value")
                                    .ok_or(CAPTURE_ERROR)?
                                    .take();
                                let text = serde_json::from_value::<Text>(body)
                                    .map_err(|_| CAPTURE_ERROR)?;
                                Ok(Capture { context, text })
                            });
                            let _ = output.send(capture);
                        },
                    )
                    .is_err()
                    {
                        let _ = result.send(Err(CAPTURE_ERROR));
                    }
                },
            )
            .is_err()
        {
            let _ = send.send(Err(CAPTURE_ERROR));
        }
    })?;
    Ok(receive)
}

pub fn evaluate(
    web: &WebView,
    context: i64,
    expression: &str,
    on_result: impl FnOnce(windows_result::Result<String>) + 'static,
) -> windows_result::Result<()> {
    web.call_dev_tools_protocol_method(
        "Runtime.evaluate",
        &json!({
            "contextId": context, "expression": expression, "returnByValue": true,
        })
        .to_string(),
        on_result,
    )
}

fn locate_worker() -> Result<(PathBuf, PathBuf, Option<PathBuf>), Failure> {
    let executable = env::current_exe().map_err(|_| WORKER_ERROR)?;
    let installed = executable
        .with_file_name("translation")
        .join("translate.mjs");
    let (translator, models) = if installed.is_file() {
        let models = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|data| data.join("BrisaBrowser").join("TranslationModels"))
            .ok_or(WORKER_ERROR)?;
        (installed, Some(models))
    } else {
        let translator = executable
            .ancestors()
            .nth(3)
            .map(|root| root.join(".toolchain/translation/translate.mjs"))
            .filter(|path| path.is_file())
            .ok_or(
                "Instale o Brisa pelo instalador ou prepare a tradução com build.ps1 -Translation.",
            )?;
        (translator, None)
    };
    let node = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .map(|directory| directory.join("nodejs").join("node.exe"))
        .filter(|path| path.is_file())
        .ok_or("Instale o Node.js para usar a tradução local.")?;
    Ok((node, translator, models))
}

pub struct Worker {
    child: Child,
    pipe: Option<JoinHandle<()>>,
    result: Receiver<Result<Vec<String>, Failure>>,
    expires: Instant,
}

impl Worker {
    pub fn start(source: &str, target: &str, text: Vec<String>) -> Result<Self, Failure> {
        let (node, translator, models) = locate_worker()?;
        let length = text.len();
        let data = serde_json::to_vec(&json!({ "source": source, "target": target, "text": text }))
            .map_err(|_| CAPTURE_ERROR)?;
        let mut command = Command::new(node);
        command.arg(translator).env_remove("NODE_OPTIONS");
        if let Some(models) = models {
            command.arg(models);
        }
        let mut child = command
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| WORKER_ERROR)?;
        let input = child.stdin.take();
        let output = child.stdout.take();
        let (send, result) = mpsc::channel();
        let pipe = thread::Builder::new()
            .name("brisa-translation".into())
            .spawn(move || {
                let read = || -> Result<Vec<String>, Failure> {
                    input
                        .ok_or(WORKER_ERROR)?
                        .write_all(&data)
                        .map_err(|_| WORKER_ERROR)?;
                    let mut bytes = Vec::new();
                    output
                        .ok_or(WORKER_ERROR)?
                        .take(RESULT_LIMIT + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| WORKER_ERROR)?;
                    if bytes.len() as u64 > RESULT_LIMIT {
                        return Err(WORKER_ERROR);
                    }
                    let translated: Vec<String> =
                        serde_json::from_slice(&bytes).map_err(|_| WORKER_ERROR)?;
                    if translated.len() != length {
                        return Err(WORKER_ERROR);
                    }
                    Ok(translated)
                };
                let _ = send.send(read());
            });
        match pipe {
            Ok(pipe) => Ok(Self {
                child,
                pipe: Some(pipe),
                result,
                expires: Instant::now() + Duration::from_secs(95),
            }),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(WORKER_ERROR)
            }
        }
    }

    pub fn poll(&self) -> Option<Result<Vec<String>, Failure>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) if Instant::now() < self.expires => None,
            Err(_) => Some(Err(WORKER_ERROR)),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The reaped process has closed both pipes, so the reader can finish.
        if let Some(pipe) = self.pipe.take() {
            let _ = pipe.join();
        }
    }
}
