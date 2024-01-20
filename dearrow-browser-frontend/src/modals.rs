use std::rc::Rc;

use yew::prelude::*;
use chrono::{NaiveDateTime, DateTime};

use crate::contexts::{StatusContext, WindowContext};
use crate::utils::render_datetime;
use crate::built_info;


#[function_component]
pub fn StatusModal() -> Html {
    let window_context: Rc<WindowContext> = use_context().expect("WindowContext should be defined");
    let status: StatusContext = use_context().expect("StatusContext should be defined");

    let errors_url: Rc<AttrValue> = use_memo(window_context, |wc| wc.origin.join("/api/errors").expect("should be able to create errors API URL").as_str().to_owned().into());

    html! {
        <div id="status-modal">
            <h2>{"About DeArrow Browser"}</h2>
            <div id="status-modal-client">
                <h3>{"Client information"}</h3>
                <h4>{"Build info"}</h4>
                <table>
                    <tr>
                        <th>{"Version"}</th>
                        <td>{built_info::PKG_VERSION}</td>
                    </tr>
                    <tr>
                        <th>{"Git hash"}</th>
                        <td>
                            if let Some(ref hash) = built_info::GIT_COMMIT_HASH {
                                <a href={format!("https://github.com/mini-bomba/DeArrowBrowser/commit/{hash}")} target="_blank">{&hash[..8]}</a>
                                if built_info::GIT_DIRTY == Some(true) {
                                    {" "}<b>{"+ uncommitted changes"}</b>
                                }
                            } else {
                                <em>{"Unknown"}</em>
                            }
                        </td>
                    </tr>
                    <tr>
                        <th>{"Build date"}</th>
                        <td>
                            if let Ok(dt) = DateTime::parse_from_rfc2822(built_info::BUILT_TIME_UTC) {
                                {render_datetime(dt.into())}
                            } else {
                                <em>{"Unknown"}</em>
                            }
                        </td>
                    </tr>
                </table>
            </div>
            <div id="status-modal-server">
                <h3>{"Server information"}</h3>
                if let Some(status) = status {
                    <h4>{"Build info"}</h4>
                    <table>
                        <tr>
                            <th>{"Version"}</th>
                            <td>{status.server_version.clone()}</td>
                        </tr>
                        <tr>
                            <th>{"Git hash"}</th>
                            <td>
                                if let Some(ref hash) = status.server_git_hash {
                                    <a href={format!("https://github.com/mini-bomba/DeArrowBrowser/commit/{hash}")} target="_blank">{&hash[..8]}</a>
                                    if status.server_git_dirty == Some(true) {
                                        {" "}<b>{"+ uncommitted changes"}</b>
                                    }
                                } else {
                                    <em>{"Unknown"}</em>
                                }
                            </td>
                        </tr>
                        <tr>
                            <th>{"Build date"}</th>
                            <td>
                                if let Some(dt) = status.server_build_timestamp.and_then(|t| NaiveDateTime::from_timestamp_opt(t, 0)) {
                                    {render_datetime(dt.and_utc())}
                                } else {
                                    <em>{"Unknown"}</em>
                                }
                            </td>
                        </tr>
                    </table>
                    <h4>{"Server status"}</h4>
                    <table>
                        <tr>
                            <th>{"Server started at"}</th>
                            <td>
                                if let Some(dt) = NaiveDateTime::from_timestamp_opt(status.server_startup_timestamp, 0) {
                                    {render_datetime(dt.and_utc())}
                                } else {
                                    <em>{"Failed to parse"}</em>
                                }
                            </td>
                        </tr>
                        <tr>
                            <th>{"Last update"}</th>
                            <td>
                                if let Some(dt) = NaiveDateTime::from_timestamp_millis(status.last_updated) {
                                    {render_datetime(dt.and_utc())}
                                    if status.updating_now {
                                        <b>{", update in progress"}</b>
                                    }
                                } else {
                                    <em>{"Failed to parse"}</em>
                                }
                            </td>
                        </tr>
                        <tr>
                            <th>{"DB snapshot taken at"}</th>
                            <td>
                                if let Some(dt) = NaiveDateTime::from_timestamp_millis(status.last_modified) {
                                    {render_datetime(dt.and_utc())}
                                } else {
                                    <em>{"Failed to parse"}</em>
                                }
                            </td>
                        </tr>
                        <tr>
                            <th>{"Title count"}</th>
                            <td>{status.titles}</td>
                        </tr>
                        <tr>
                            <th>{"Thumbnail count"}</th>
                            <td>{status.thumbnails}</td>
                        </tr>
                        <tr>
                            <th>{"Username count"}</th>
                            <td>{status.usernames}</td>
                        </tr>
                        <tr>
                            <th>{"VIPs"}</th>
                            <td>{status.vip_users}</td>
                        </tr>
                        <tr>
                            <th>{"Unique strings"}</th>
                            <td>
                                if let Some(count) = status.string_count {
                                    {count}
                                } else {
                                    <em>{"Unknown"}</em>
                                }
                            </td>
                        </tr>
                        <tr>
                            <th>{"Parse errors"}</th>
                            <td>
                                {status.errors}{" "}
                                <a href={(*errors_url).clone()} target="_blank">{"(view)"}</a>
                            </td>
                        </tr>
                    </table>
                } else {
                    <em>{"Loading..."}</em>
                }
            </div>
        </div>
    }
}
