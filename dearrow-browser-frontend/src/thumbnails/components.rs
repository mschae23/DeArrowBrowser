/* This file is part of the DeArrow Browser project - https://github.com/mini-bomba/DeArrowBrowser
*
*  Copyright (C) 2024 mini_bomba
*  
*  This program is free software: you can redistribute it and/or modify
*  it under the terms of the GNU Affero General Public License as published by
*  the Free Software Foundation, either version 3 of the License, or
*  (at your option) any later version.
*
*  This program is distributed in the hope that it will be useful,
*  but WITHOUT ANY WARRANTY; without even the implied warranty of
*  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
*  GNU Affero General Public License for more details.
*
*  You should have received a copy of the GNU Affero General Public License
*  along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::rc::Rc;

use gloo_console::{error, log};
use reqwest::Url;
use yew::prelude::*;
use yew_hooks::{use_async_with_options, UseAsyncHandle, UseAsyncOptions};

use crate::{components::modals::{thumbnail::ThumbnailModal, ModalMessage}, hooks::use_async_suspension, thumbnails::worker_api::{ThumbnailWorkerRequest, WorkerSetting}, utils::RcEq, ModalRendererControls, SettingsContext};

use super::{common::{ThumbgenStats, ThumbnailKey}, local::{LocalBlobLink, LocalThumbGenerator}, remote::{Error, RemoteBlobLink, ThumbnailWorker}};

#[derive(Clone, Eq, PartialEq)]
pub enum Thumbgen {
    Remote(ThumbnailWorker),
    Local{
        gen: LocalThumbGenerator,
        /// Error from the attempt to initialize the remote thumbnail worker
        error: RcEq<super::remote::Error>,
    },
}

#[derive(Clone, Eq, PartialEq)]
pub enum ThumbnailUrl {
    Local(Rc<LocalBlobLink>),
    Remote(Rc<RemoteBlobLink>),
}

impl ThumbnailUrl {
    fn get_url(&self) -> &str {
        match self {
            Self::Local(url) => url.inner_url(),
            Self::Remote(url) => url.inner_url(),
        }
    }
}

impl Thumbgen {
    pub async fn get_thumbnail(&self, key: &ThumbnailKey) -> Result<ThumbnailUrl, Error> {
        match self {
            Self::Remote(worker) => worker.get_thumbnail(key.clone()).await.map(|t| ThumbnailUrl::Remote(Rc::new(t))),
            Self::Local { gen, .. } => gen.get_thumb(key).await.map(ThumbnailUrl::Local).map_err(|e| Error::Remote(e.into())),
        }
    }

    pub async fn get_stats(&self) -> Result<ThumbgenStats, Error> {
        match self {
            Self::Remote(worker) => worker.get_stats().await,
            Self::Local { gen, .. } => Ok(ThumbgenStats { cache_stats: gen.get_stats(), worker_stats: None }),
        }
    }
}

pub type ThumbgenContext = Option<Thumbgen>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ThumbgenRefreshContext(pub u8);

impl ThumbgenRefreshContext {
    fn bump(self) -> Self {
        ThumbgenRefreshContext(self.0.wrapping_add(1))
    }
}

#[derive(Properties, PartialEq)]
pub struct ThumbnailGeneratorProviderProps {
    pub children: Html
}

#[function_component]
pub fn ThumbgenProvider(props: &ThumbnailGeneratorProviderProps) -> Html {
    let settings_context: SettingsContext = use_context().expect("ThumbnailProvider must be placed under a SettingsContext provider");
    let settings = settings_context.settings();
    let thumgen_state: UseAsyncHandle<Thumbgen, ()> = use_async_with_options(async move {
        Ok(match ThumbnailWorker::new().await {
            Ok(worker) => Thumbgen::Remote(worker),
            Err(err) => Thumbgen::Local {
                gen: LocalThumbGenerator::new(),
                error: RcEq::new(err),
            },
        })
    }, UseAsyncOptions::enable_auto());
    let refresh_state = use_state(|| ThumbgenRefreshContext(0));

    // Thumbgen API URL updates
    use_memo((thumgen_state.data.clone(), settings.thumbgen_api_base_url.clone()), |(thumbgen, api_base_url)| {
        match thumbgen {
            None => (),
            Some(Thumbgen::Remote(worker)) => {
                if let Err(e) = worker.post_request(ThumbnailWorkerRequest::SettingUpdated {
                    setting: WorkerSetting::ThumbgenBaseUrl(api_base_url.to_string())
                }) {
                    error!(format!("Failed to notify Thumbgen::Remote about a thumbgen API base URL change: {e}"));
                }
            },
            Some(Thumbgen::Local { r#gen, .. }) => {
                let mut url = match Url::parse(api_base_url) {
                    Ok(url) => url,
                    Err(e) => return error!(format!("Failed to parse new ThumbgenBaseUrl: {e}")),
                };
                {
                    let Ok(mut path) = url.path_segments_mut() else {
                        return error!(format!("Failed to append API endpoint to new ThumbgenBaseUrl: {api_base_url} cannot be a base"))
                    };
                    path.extend(&["api", "v1", "getThumbnail"]);
                };
                r#gen.set_api_url(url);
                let errors_removed = r#gen.clear_errors();
                log!(format!("Cleared {errors_removed} error entries after updating thumbgen API URL"));
            }
        };
        refresh_state.set(refresh_state.bump());
    });

    html! {
        <ContextProvider<ThumbgenContext> context={thumgen_state.data.clone()}>
        <ContextProvider<ThumbgenRefreshContext> context={*refresh_state}>
            { props.children.clone() }
        </ContextProvider<ThumbgenRefreshContext>>
        </ContextProvider<ThumbgenContext>>
    }
}

#[derive(Properties, PartialEq)]
pub struct BaseThumbnailProps {
    pub thumb_key: ThumbnailKey,
}

#[function_component]
pub fn BaseThumbnail(props: &BaseThumbnailProps) -> HtmlResult {
    let generator: ThumbgenContext = use_context().expect("BaseThumbnail must be run under a ThumbnailGeneratorProvider");
    let refresher: ThumbgenRefreshContext = use_context().expect("BaseThumbnail must be run under a ThumbnailGeneratorProvider");
    let thumbnail = use_async_suspension(|(generator, key, _)| async move {
        Some(generator?.get_thumbnail(&key).await)
    }, (generator, props.thumb_key.clone(), refresher))?;
    
    Ok(match *thumbnail {
        None => html! { <span class="thumbnail-error">{"Waiting for thumbnail generator..."}</span>},
        Some(Err(ref err)) => html! { <span class="thumbnail-error">{format!("{err:?}")}</span> },
        Some(Ok(ref url)) => html! { <img class="thumbnail" src={Rc::from(url.get_url())} /> },
    })
}

#[derive(Properties, PartialEq, Clone)]
pub struct ThumbnailProps {
    pub video_id: Rc<str>,
    /// none means original thumb
    pub timestamp: Option<f64>,
}

#[function_component]
pub fn UnwrappedThumbnail(props: &ThumbnailProps) -> Html {
    let timestamp: Rc<Rc<str>> = use_memo(props.clone(), |props| {
        match props.timestamp {
            None => format!("https://img.youtube.com/vi/{}/maxresdefault.jpg", props.video_id),
            Some(t) => format!("{t}"),
        }.into()
    });
    if props.timestamp.is_none() {
        html! {
            <img class="thumbnail" src={(*timestamp).clone()} />
        }
    } else {
        let fallback = html! {<span class="thumbnail-error">{"Generating thumbnail..."}</span>};
        let thumb_key = ThumbnailKey {
            video_id: props.video_id.clone(),
            timestamp: (*timestamp).clone(),
        };
        html! {
            <Suspense {fallback}>
                <BaseThumbnail {thumb_key} />
            </Suspense>
        }
    }
}

#[function_component]
pub fn Thumbnail(props: &ThumbnailProps) -> Html {
    let modal_controls: ModalRendererControls = use_context().expect("ModalRendererControls should be available");
    let onclick = {
        let props = props.clone();
        Callback::from(move |_| {
            modal_controls.emit(ModalMessage::Open(html! {
                <ThumbnailModal ..props.clone() />
            }));
        })
    };
    html! {
        <div class="thumbnail-container clickable" {onclick}>
            <UnwrappedThumbnail ..props.clone() />
        </div>
    }
}
