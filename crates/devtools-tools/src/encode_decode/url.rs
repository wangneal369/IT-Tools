//! #7 URL 编解码与解析。
//!
//! 结构拆解（`url`）与保序键值对查询参数（`Vec` 支持同名重复键），
//! `%20` / `+` 可选；组件级与完整 URL 的 Percent 编解码支持逐行模式。

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use url::Url;

use crate::util::{invalid_syntax, process_lines};

use devtools_core::ToolError;

/// RFC 3986 unreserved 字符（`A-Z a-z 0-9 - . _ ~`）之外全部编码。
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// 完整 URL：保留 reserved 字符，使结构保持可读。
const FULL_URL: &AsciiSet = &COMPONENT
    .remove(b':')
    .remove(b'/')
    .remove(b'?')
    .remove(b'#')
    .remove(b'[')
    .remove(b']')
    .remove(b'@')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=');

/// 操作模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlMode {
    Parse,
    Encode,
    Decode,
}

/// Percent 编码作用对象。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlTarget {
    /// 单个组件（保留字符也会被编码）。
    Component,
    /// 完整 URL（保留 `:/?#[]@!$&'()*+,;=`）。
    FullUrl,
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlCodecRequest {
    pub input: String,
    pub mode: UrlMode,
    pub target: UrlTarget,
    /// 空格以 `+` 表示（仅组件模式编码 / 解码生效）。
    pub space_as_plus: bool,
    pub line_by_line: bool,
}

/// 保序查询参数（允许重复键）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryParam {
    pub key: String,
    pub value: String,
}

/// URL 结构拆解。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlParts {
    pub scheme: String,
    pub username: String,
    pub password: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
    pub origin: String,
}

/// 处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlCodecResponse {
    /// 规范化的 URL（Parse）或编码/解码后的文本。
    pub output: String,
    /// 结构拆解（仅 Parse 模式）。
    pub parts: Option<UrlParts>,
    /// 保序查询参数（仅 Parse 模式）。
    pub params: Vec<QueryParam>,
    /// 重新编码后的查询串（保留重复键与顺序）。
    pub query_string: String,
}

/// 执行 URL 解析或编解码。
pub fn run(req: &UrlCodecRequest) -> Result<UrlCodecResponse, ToolError> {
    match req.mode {
        UrlMode::Parse => parse(req),
        UrlMode::Encode => transform(req, true),
        UrlMode::Decode => transform(req, false),
    }
}

fn parse(req: &UrlCodecRequest) -> Result<UrlCodecResponse, ToolError> {
    let input = req.input.trim();
    if input.is_empty() {
        return Err(invalid_syntax("URL 输入为空"));
    }
    let url = Url::parse(input).map_err(|err| {
        invalid_syntax(format!("URL 解析失败：{err}"))
            .with_hint("请补全协议，例如 https://example.com/path?x=1")
    })?;
    let params: Vec<QueryParam> = url
        .query_pairs()
        .map(|(key, value)| QueryParam {
            key: key.into_owned(),
            value: value.into_owned(),
        })
        .collect();
    let query_string = encode_params(&params, req.space_as_plus);

    Ok(UrlCodecResponse {
        output: url.as_str().to_string(),
        parts: Some(UrlParts {
            scheme: url.scheme().to_string(),
            username: url.username().to_string(),
            password: url.password().map(str::to_string),
            host: url.host_str().map(str::to_string),
            port: url.port(),
            path: url.path().to_string(),
            query: url.query().map(str::to_string),
            fragment: url.fragment().map(str::to_string),
            origin: url.origin().ascii_serialization(),
        }),
        params,
        query_string,
    })
}

fn encode_params(params: &[QueryParam], space_as_plus: bool) -> String {
    if space_as_plus {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for param in params {
            serializer.append_pair(&param.key, &param.value);
        }
        serializer.finish()
    } else {
        params
            .iter()
            .map(|param| {
                format!(
                    "{}={}",
                    utf8_percent_encode(&param.key, COMPONENT),
                    utf8_percent_encode(&param.value, COMPONENT)
                )
            })
            .collect::<Vec<_>>()
            .join("&")
    }
}

fn transform(req: &UrlCodecRequest, encode: bool) -> Result<UrlCodecResponse, ToolError> {
    let apply = |line: &str| -> Result<String, ToolError> {
        if encode {
            let mut out = utf8_percent_encode(line, target_set(req.target)).to_string();
            if req.space_as_plus && req.target == UrlTarget::Component {
                out = out.replace("%20", "+");
            }
            Ok(out)
        } else {
            let prepared = if req.space_as_plus && req.target == UrlTarget::Component {
                std::borrow::Cow::Owned(line.replace('+', " "))
            } else {
                std::borrow::Cow::Borrowed(line)
            };
            percent_decode_str(&prepared)
                .decode_utf8()
                .map(|text| text.into_owned())
                .map_err(|_| invalid_syntax("Percent 解码结果不是合法的 UTF-8 文本"))
        }
    };

    let output = if req.line_by_line {
        process_lines(&req.input, |line, _| apply(line))?
    } else {
        apply(&req.input)?
    };
    Ok(UrlCodecResponse {
        output,
        parts: None,
        params: Vec::new(),
        query_string: String::new(),
    })
}

fn target_set(target: UrlTarget) -> &'static AsciiSet {
    match target {
        UrlTarget::Component => COMPONENT,
        UrlTarget::FullUrl => FULL_URL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str, mode: UrlMode, target: UrlTarget) -> UrlCodecRequest {
        UrlCodecRequest {
            input: input.to_string(),
            mode,
            target,
            space_as_plus: false,
            line_by_line: false,
        }
    }

    #[test]
    fn parse_decomposes_all_parts() {
        let resp = run(&request(
            "https://user:pass@example.com:8443/a/b?x=1&x=2&y=hello%20world#frag",
            UrlMode::Parse,
            UrlTarget::FullUrl,
        ))
        .unwrap();
        let parts = resp.parts.unwrap();
        assert_eq!(parts.scheme, "https");
        assert_eq!(parts.username, "user");
        assert_eq!(parts.password.as_deref(), Some("pass"));
        assert_eq!(parts.host.as_deref(), Some("example.com"));
        assert_eq!(parts.port, Some(8443));
        assert_eq!(parts.path, "/a/b");
        assert_eq!(parts.fragment.as_deref(), Some("frag"));
        assert_eq!(parts.origin, "https://example.com:8443");
    }

    #[test]
    fn duplicate_query_keys_keep_order() {
        let resp = run(&request(
            "https://example.com/?a=1&b=2&a=3",
            UrlMode::Parse,
            UrlTarget::FullUrl,
        ))
        .unwrap();
        assert_eq!(resp.params.len(), 3);
        assert_eq!(resp.params[0].key, "a");
        assert_eq!(resp.params[0].value, "1");
        assert_eq!(resp.params[1].key, "b");
        assert_eq!(resp.params[2].value, "3");
        assert_eq!(resp.query_string, "a=1&b=2&a=3");
    }

    #[test]
    fn query_space_mode_percent_or_plus() {
        let mut req = request(
            "https://example.com/?q=hello%20world",
            UrlMode::Parse,
            UrlTarget::FullUrl,
        );
        req.space_as_plus = true;
        let resp = run(&req).unwrap();
        assert_eq!(resp.params[0].value, "hello world");
        assert_eq!(resp.query_string, "q=hello+world");

        req.space_as_plus = false;
        let resp = run(&req).unwrap();
        assert_eq!(resp.query_string, "q=hello%20world");
    }

    #[test]
    fn component_encode_and_decode_round_trip() {
        let encoded = run(&request("a b&c=中", UrlMode::Encode, UrlTarget::Component)).unwrap();
        assert_eq!(encoded.output, "a%20b%26c%3D%E4%B8%AD");
        let decoded = run(&request(
            &encoded.output,
            UrlMode::Decode,
            UrlTarget::Component,
        ))
        .unwrap();
        assert_eq!(decoded.output, "a b&c=中");
    }

    #[test]
    fn full_url_encoding_keeps_reserved_characters() {
        let input = "https://example.com/a b?x=1&y=2";
        let encoded = run(&request(input, UrlMode::Encode, UrlTarget::FullUrl)).unwrap();
        assert_eq!(encoded.output, "https://example.com/a%20b?x=1&y=2");
    }

    #[test]
    fn plus_mode_encodes_space_as_plus() {
        let mut req = request("a b", UrlMode::Encode, UrlTarget::Component);
        req.space_as_plus = true;
        assert_eq!(run(&req).unwrap().output, "a+b");
        let mut req = request("a+b", UrlMode::Decode, UrlTarget::Component);
        req.space_as_plus = true;
        assert_eq!(run(&req).unwrap().output, "a b");
    }

    #[test]
    fn line_by_line_processes_each_line() {
        let mut req = request("a b\nc&d\n", UrlMode::Encode, UrlTarget::Component);
        req.line_by_line = true;
        assert_eq!(run(&req).unwrap().output, "a%20b\nc%26d\n");
    }

    #[test]
    fn relative_url_is_rejected_with_hint() {
        let err = run(&request(
            "example.com/x",
            UrlMode::Parse,
            UrlTarget::FullUrl,
        ))
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        assert!(err.hint.is_some());
    }

    #[test]
    fn invalid_utf8_after_decode_is_rejected() {
        let err = run(&request("%FF", UrlMode::Decode, UrlTarget::Component)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}
