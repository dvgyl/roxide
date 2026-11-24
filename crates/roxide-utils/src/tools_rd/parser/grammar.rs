#[derive(Clone, Copy)]
pub(super) enum Mode {
    Text,
    Code,
    Verbatim,
    Equation,
}

impl Mode {
    pub(super) fn tag(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Code => "RCODE",
            Self::Verbatim | Self::Equation => "VERB",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Item {
    None,
    Marker,
    Pair,
}

#[derive(Clone, Copy)]
pub(super) struct Grammar {
    pub(super) mode: Mode,
    pub(super) item: Item,
    pub(super) sections: bool,
}

impl Grammar {
    pub(super) const TEXT: Self = Self {
        mode: Mode::Text,
        item: Item::None,
        sections: false,
    };
}

pub(super) enum Placement {
    Inline,
    Section,
    Anywhere,
}

pub(super) struct Spec {
    pub(super) args: &'static [Mode],
    pub(super) item: Item,
    pub(super) placement: Placement,
    pub(super) option: bool,
    pub(super) optional_last: bool,
}

impl Spec {
    const fn inline(args: &'static [Mode]) -> Self {
        Self {
            args,
            item: Item::None,
            placement: Placement::Inline,
            option: false,
            optional_last: false,
        }
    }

    const fn section(args: &'static [Mode]) -> Self {
        Self {
            placement: Placement::Section,
            ..Self::inline(args)
        }
    }

    pub(super) fn get(tag: &str, item: Item) -> Option<Self> {
        use Mode::{Code, Equation, Text, Verbatim};
        Some(match tag {
            "\\name" | "\\alias" | "\\Rdversion" | "\\synopsis" => Self::section(&[Verbatim]),
            "\\title" | "\\description" | "\\details" | "\\note" | "\\author" | "\\references"
            | "\\seealso" | "\\keyword" | "\\concept" | "\\docType" | "\\encoding" | "\\format"
            | "\\source" => Self::section(&[Text]),
            "\\arguments" | "\\value" => Self {
                item: Item::Pair,
                ..Self::section(&[Text])
            },
            "\\usage" | "\\examples" => Self::section(&[Code]),
            "\\section" => Self::section(&[Text, Text]),
            "\\code" | "\\special" | "\\donttest" | "\\dontshow" | "\\testonly" => {
                Self::inline(&[Code])
            }
            "\\Sexpr" => Self {
                placement: Placement::Anywhere,
                option: true,
                ..Self::inline(&[Code])
            },
            "\\preformatted" | "\\verb" | "\\url" | "\\env" | "\\out" | "\\dontrun" | "\\kbd" => {
                Self::inline(&[Verbatim])
            }
            "\\emph" | "\\strong" | "\\bold" | "\\sQuote" | "\\dQuote" | "\\pkg" | "\\file"
            | "\\option" | "\\command" | "\\dfn" | "\\cite" | "\\email" | "\\acronym" | "\\var"
            | "\\abbr" => Self::inline(&[Text]),
            "\\link" | "\\linkS4class" => Self {
                option: true,
                ..Self::inline(&[Text])
            },
            "\\itemize" | "\\enumerate" => Self {
                item: Item::Marker,
                ..Self::inline(&[Text])
            },
            "\\describe" => Self {
                item: Item::Pair,
                ..Self::inline(&[Text])
            },
            "\\href" => Self::inline(&[Verbatim, Text]),
            "\\eqn" | "\\deqn" => Self {
                optional_last: true,
                ..Self::inline(&[Equation, Verbatim])
            },
            "\\enc" | "\\tabular" | "\\method" | "\\S3method" | "\\S4method" | "\\if" => {
                Self::inline(&[Text, Text])
            }
            "\\ifelse" => Self::inline(&[Text, Text, Text]),
            "\\R" | "\\dots" | "\\ldots" | "\\cr" | "\\tab" => Self::inline(&[]),
            "\\item" => match item {
                Item::None => return None,
                Item::Marker => Self::inline(&[]),
                Item::Pair => Self::inline(&[Text, Text]),
            },
            _ => return None,
        })
    }
}
