use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::placeholders::I_PLACEHOLDER;
use sicompass_sdk::provider::Provider;
use std::path::Path;
use std::sync::OnceLock;

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

// ---------------------------------------------------------------------------
// Tutorial content tree
// ---------------------------------------------------------------------------

/// A node in the tutorial content tree (mirrors the TypeScript `Section` type).
enum Node {
    Leaf(&'static str),
    Branch { key: &'static str, children: &'static [Node] },
}

use Node::{Branch, Leaf};

fn lorem_ipsum() -> &'static str {
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium."
}

// Static tree matching tutorial.ts `sections` array.
// Asset paths (TEXTURE_JPG, SF_JSON) are substituted at runtime via TutorialProvider.

static SECTIONS: &[Node] = &[
    Branch { key: "tutorial-branch-001",
        children: &[
            Leaf("tutorial-leaf-001"),
            Leaf("tutorial-leaf-002"),
            Leaf("tutorial-leaf-003"),
            Leaf("tutorial-leaf-004"),
            Leaf("tutorial-leaf-005"),
            Leaf("tutorial-leaf-006"),
        ],
    },
    Branch { key: "tutorial-branch-002",
        children: &[
            Leaf("tutorial-leaf-007"),
            Branch { key: "tutorial-branch-003",
                children: &[
                    Leaf("tutorial-leaf-008"),
                    Leaf("tutorial-leaf-009"),
                    Leaf("tutorial-leaf-010"),
                    Leaf("tutorial-leaf-011"),
                    Leaf("tutorial-leaf-012"),
                    Leaf("tutorial-leaf-013"),
                    Leaf("tutorial-leaf-014"),
                    Leaf("tutorial-leaf-015"),
                    Leaf("tutorial-leaf-016"),
                ],
            },
            Branch { key: "tutorial-branch-004",
                children: &[
                    Leaf("tutorial-leaf-017"),
                    Leaf("tutorial-leaf-018"),
                    Leaf("tutorial-leaf-019"),
                    Leaf("tutorial-leaf-020"),
                    Leaf("tutorial-leaf-021"),
                    Leaf("tutorial-leaf-022"),
                ],
            },
            Branch { key: "tutorial-branch-047",
                children: &[
                    Leaf("tutorial-leaf-243"),
                    Leaf("tutorial-leaf-244"),
                    Leaf("tutorial-leaf-245"),
                    Leaf("tutorial-leaf-246"),
                    Leaf("tutorial-leaf-247"),
                    Leaf("tutorial-leaf-248"),
                    Leaf("tutorial-leaf-249"),
                ],
            },
        ],
    },
    Branch { key: "tutorial-branch-005",
        children: &[
            Leaf("tutorial-leaf-023"),
            Leaf("tutorial-leaf-024"),
            Leaf("tutorial-leaf-025"),
            Leaf("tutorial-leaf-026"),
            Leaf("tutorial-leaf-256"),
        ],
    },
    Branch { key: "tutorial-branch-006",
        children: &[
            Leaf("tutorial-leaf-027"),
            Leaf("tutorial-leaf-028"),
            Leaf("tutorial-leaf-029"),
            Leaf("tutorial-leaf-030"),
            Leaf("tutorial-leaf-031"),
            Leaf("tutorial-leaf-032"),
            Leaf("tutorial-leaf-033"),
            Leaf("tutorial-leaf-257"),
        ],
    },
    Branch { key: "tutorial-branch-007",
        children: &[
            Leaf("tutorial-leaf-034"),
            Leaf("tutorial-leaf-035"),
            Leaf("tutorial-leaf-036"),
            Branch { key: "tutorial-branch-008",
                children: &[
                    Leaf("tutorial-leaf-037"),
                    Leaf("tutorial-leaf-038"),
                    Leaf("tutorial-leaf-039"),
                    Leaf("tutorial-leaf-040"),
                    Leaf("tutorial-leaf-041"),
                    Leaf("tutorial-leaf-042"),
                    Leaf("tutorial-leaf-043"),
                    Leaf("tutorial-leaf-044"),
                ],
            },
            Branch { key: "tutorial-branch-009",
                children: &[
                    Leaf("tutorial-leaf-045"),
                    Leaf("tutorial-leaf-046"),
                    Leaf("tutorial-leaf-047"),
                    Leaf("tutorial-leaf-048"),
                ],
            },
            Branch { key: "tutorial-branch-010",
                children: &[
                    Leaf("tutorial-leaf-049"),
                    Leaf("tutorial-leaf-050"),
                    Leaf("tutorial-leaf-051"),
                    Leaf("tutorial-leaf-052"),
                ],
            },
        ],
    },
    Branch { key: "tutorial-branch-011",
        children: &[
            Leaf("tutorial-leaf-053"),
            Leaf("tutorial-leaf-054"),
            Leaf("tutorial-leaf-055"),
            Leaf("tutorial-leaf-056"),
            Leaf("tutorial-leaf-057"),
            Leaf("tutorial-leaf-058"),
            Leaf("tutorial-leaf-059"),
            Leaf("tutorial-leaf-060"),
        ],
    },
    Branch { key: "tutorial-branch-012",
        children: &[
            Leaf("tutorial-leaf-061"),
            Leaf("tutorial-leaf-062"),
            Leaf("tutorial-leaf-063"),
            Branch { key: "tutorial-branch-013", children: &[
                Leaf("tutorial-leaf-064"),
                Leaf("tutorial-leaf-065"),
                Leaf("tutorial-leaf-066"),
                Leaf("tutorial-leaf-067"),
                Leaf("tutorial-leaf-068"),
                Leaf("tutorial-leaf-069"),
                Leaf("tutorial-leaf-070"),
                Leaf("tutorial-leaf-071"),
            ]},
            Branch { key: "tutorial-branch-014", children: &[
                Leaf("tutorial-leaf-072"),
                Leaf("tutorial-leaf-073"),
                Leaf("tutorial-leaf-074"),
                Leaf("tutorial-leaf-075"),
                Leaf("tutorial-leaf-076"),
                Leaf("tutorial-leaf-077"),
            ]},
            Branch { key: "tutorial-branch-015", children: &[
                Leaf("tutorial-leaf-078"),
                Leaf("tutorial-leaf-079"),
                Leaf("tutorial-leaf-080"),
                Leaf("tutorial-leaf-081"),
                Leaf("tutorial-leaf-082"),
                Leaf("tutorial-leaf-083"),
            ]},
            Branch { key: "tutorial-branch-016", children: &[
                Leaf("tutorial-leaf-084"),
                Leaf("tutorial-leaf-085"),
                Leaf("tutorial-leaf-086"),
                Leaf("tutorial-leaf-087"),
                Leaf("tutorial-leaf-088"),
                Leaf("tutorial-leaf-089"),
                Leaf("tutorial-leaf-090"),
                Leaf("tutorial-leaf-091"),
            ]},
            Branch { key: "tutorial-branch-017", children: &[
                Leaf("tutorial-leaf-092"),
                Leaf("tutorial-leaf-093"),
                Leaf("tutorial-leaf-094"),
                Leaf("tutorial-leaf-095"),
                Leaf("tutorial-leaf-096"),
                Leaf("tutorial-leaf-097"),
            ]},
            Branch { key: "tutorial-branch-018", children: &[
                Leaf("tutorial-leaf-098"),
                Leaf("tutorial-leaf-255"),
                Leaf("tutorial-leaf-099"),
                Leaf("tutorial-leaf-100"),
                Leaf("tutorial-leaf-101"),
                Leaf("tutorial-leaf-102"),
            ]},
            Branch { key: "tutorial-branch-019", children: &[
                Leaf("tutorial-leaf-103"),
                Leaf("tutorial-leaf-104"),
                Leaf("tutorial-leaf-105"),
                Leaf("tutorial-leaf-106"),
                Leaf("tutorial-leaf-107"),
            ]},
            Branch { key: "tutorial-branch-020", children: &[
                Leaf("tutorial-leaf-108"),
                Leaf("tutorial-leaf-109"),
                Leaf("tutorial-leaf-110"),
                Leaf("tutorial-leaf-111"),
                Leaf("tutorial-leaf-112"),
                Leaf("tutorial-leaf-113"),
                Leaf("tutorial-leaf-114"),
                Leaf("tutorial-leaf-115"),
            ]},
            Branch { key: "tutorial-branch-021", children: &[
                Leaf("tutorial-leaf-116"),
                Leaf("tutorial-leaf-117"),
                Leaf("tutorial-leaf-118"),
                Leaf("tutorial-leaf-119"),
                Leaf("tutorial-leaf-120"),
                Leaf("tutorial-leaf-121"),
                Leaf("tutorial-leaf-122"),
                Leaf("tutorial-leaf-123"),
            ]},
            Branch { key: "tutorial-branch-022", children: &[
                Leaf("tutorial-leaf-124"),
                Leaf("tutorial-leaf-125"),
                Leaf("tutorial-leaf-126"),
                Leaf("tutorial-leaf-127"),
                Leaf("tutorial-leaf-128"),
                Leaf("tutorial-leaf-129"),
                Leaf("tutorial-leaf-130"),
            ]},
        ],
    },
    // "Interactive Elements" section - leaf nodes with tag examples.
    // Asset paths (<image> and <link>) are filled in at runtime by TutorialProvider.
    Branch { key: "tutorial-branch-023",
        children: &[
            Leaf("tutorial-leaf-131"),
            Leaf("tutorial-leaf-132"),
            Leaf("tutorial-leaf-133"),
            Leaf("tutorial-leaf-134"),
            Branch { key: "tutorial-branch-024", children: &[
                Leaf("tutorial-leaf-135"),
                Leaf("tutorial-leaf-136"),
                Leaf("tutorial-leaf-137"),
            ]},
            Branch { key: "tutorial-branch-025", children: &[
                Leaf("tutorial-leaf-138"),
            ]},
            Leaf("tutorial-leaf-139"),
            Leaf("tutorial-leaf-140"),
            Branch { key: "tutorial-branch-026", children: &[
                Leaf(I_PLACEHOLDER),
            ]},
            Branch { key: "tutorial-branch-027", children: &[
                Leaf("tutorial-leaf-141"),
                Leaf("tutorial-leaf-142"),
                Leaf("tutorial-leaf-143"),
            ]},
            Leaf("tutorial-leaf-144"),
            Leaf("tutorial-leaf-145"),
            // TEXTURE_JPG placeholder, replaced by TutorialProvider::make_interactive_elements
            Leaf("tutorial-leaf-146"),
            Leaf("tutorial-leaf-147"),
            Leaf("tutorial-leaf-148"),
            Leaf("tutorial-leaf-149"),
            Leaf("tutorial-leaf-150"),
            // SF_JSON placeholder, replaced at runtime
            Branch { key: "tutorial-branch-028", children: &[] },
            Leaf("tutorial-leaf-151"),
            Leaf("tutorial-leaf-152"),
        ],
    },
    Branch { key: "tutorial-branch-029",
        children: &[
            Leaf("tutorial-leaf-153"),
            Branch { key: "tutorial-branch-030", children: &[
                Leaf("tutorial-leaf-154"),
                Leaf("tutorial-leaf-155"),
                Leaf("tutorial-leaf-156"),
                Leaf("tutorial-leaf-157"),
                Leaf("tutorial-leaf-158"),
            ]},
            Branch { key: "tutorial-branch-031", children: &[
                Leaf("tutorial-leaf-159"),
                Leaf("tutorial-leaf-160"),
                Leaf("tutorial-leaf-161"),
                Leaf("tutorial-leaf-162"),
                Leaf("tutorial-leaf-163"),
            ]},
            Branch { key: "tutorial-branch-032", children: &[
                Leaf("tutorial-leaf-164"),
                Leaf("tutorial-leaf-165"),
                Leaf("tutorial-leaf-166"),
                Leaf("tutorial-leaf-167"),
            ]},
            Branch { key: "tutorial-branch-048", children: &[
                Leaf("tutorial-leaf-250"),
                Leaf("tutorial-leaf-251"),
                Leaf("tutorial-leaf-252"),
                Leaf("tutorial-leaf-253"),
                Leaf("tutorial-leaf-254"),
            ]},
        ],
    },
    Branch { key: "tutorial-branch-033",
        children: &[
            Leaf("tutorial-leaf-168"),
            Leaf("tutorial-leaf-169"),
            Branch { key: "tutorial-branch-034", children: &[
                Leaf("tutorial-leaf-170"),
                Leaf("tutorial-leaf-171"),
                Leaf("tutorial-leaf-172"),
                Leaf("tutorial-leaf-173"),
                Leaf("tutorial-leaf-174"),
                Leaf("tutorial-leaf-175"),
                Leaf("tutorial-leaf-176"),
                Leaf("tutorial-leaf-177"),
                Leaf("tutorial-leaf-178"),
            ]},
            Branch { key: "tutorial-branch-035", children: &[
                Leaf("tutorial-leaf-179"),
                Leaf("tutorial-leaf-180"),
                Leaf("tutorial-leaf-181"),
                Leaf("tutorial-leaf-182"),
                Leaf("tutorial-leaf-183"),
                Leaf("tutorial-leaf-184"),
                Leaf("tutorial-leaf-185"),
                Leaf("tutorial-leaf-186"),
                Leaf("tutorial-leaf-187"),
                Leaf("tutorial-leaf-188"),
            ]},
            Branch { key: "tutorial-branch-036", children: &[
                Leaf("tutorial-leaf-189"),
                Leaf("tutorial-leaf-190"),
                Leaf("tutorial-leaf-191"),
                Leaf("tutorial-leaf-192"),
            ]},
            Branch { key: "tutorial-branch-037", children: &[
                Leaf("tutorial-leaf-193"),
                Branch { key: "tutorial-branch-038", children: &[
                    Leaf("tutorial-leaf-194"),
                    Leaf("tutorial-leaf-195"),
                    Leaf("tutorial-leaf-196"),
                    Leaf("tutorial-leaf-197"),
                ]},
                Branch { key: "tutorial-branch-039", children: &[
                    Leaf("tutorial-leaf-198"),
                    Leaf("tutorial-leaf-199"),
                    Leaf("tutorial-leaf-200"),
                    Leaf("tutorial-leaf-201"),
                ]},
                Branch { key: "tutorial-branch-040", children: &[
                    Leaf("tutorial-leaf-202"),
                    Leaf("tutorial-leaf-203"),
                    Leaf("tutorial-leaf-204"),
                    Leaf("tutorial-leaf-205"),
                ]},
                Branch { key: "tutorial-branch-041", children: &[
                    Leaf("tutorial-leaf-206"),
                    Leaf("tutorial-leaf-207"),
                    Leaf("tutorial-leaf-208"),
                    Leaf("tutorial-leaf-209"),
                ]},
                Branch { key: "tutorial-branch-042", children: &[
                    Leaf("tutorial-leaf-210"),
                    Leaf("tutorial-leaf-211"),
                    Leaf("tutorial-leaf-212"),
                    Leaf("tutorial-leaf-213"),
                ]},
                Branch { key: "tutorial-branch-043", children: &[
                    Leaf("tutorial-leaf-214"),
                    Leaf("tutorial-leaf-215"),
                    Leaf("tutorial-leaf-216"),
                ]},
                Branch { key: "tutorial-branch-044", children: &[
                    Leaf("tutorial-leaf-217"),
                    Leaf("tutorial-leaf-218"),
                ]},
            ]},
            Branch { key: "tutorial-branch-045", children: &[
                Leaf("tutorial-leaf-219"),
                Leaf("tutorial-leaf-220"),
                Leaf("tutorial-leaf-221"),
                Leaf("tutorial-leaf-222"),
                Leaf("tutorial-leaf-223"),
                Leaf("tutorial-leaf-224"),
                Leaf("tutorial-leaf-225"),
                Leaf("tutorial-leaf-226"),
                Leaf("tutorial-leaf-227"),
                Leaf("tutorial-leaf-228"),
                Leaf("tutorial-leaf-229"),
                Leaf("tutorial-leaf-230"),
                Leaf("tutorial-leaf-231"),
                Leaf("tutorial-leaf-232"),
                Leaf("tutorial-leaf-233"),
            ]},
        ],
    },
    Branch { key: "tutorial-branch-046",
        children: &[
            Leaf("tutorial-leaf-234"),
            Leaf("tutorial-leaf-235"),
            Leaf("tutorial-leaf-236"),
            Leaf("tutorial-leaf-237"),
            Leaf("tutorial-leaf-238"),
            Leaf("tutorial-leaf-239"),
            Leaf("tutorial-leaf-240"),
            Leaf("tutorial-leaf-241"),
            Leaf("tutorial-leaf-242"),
        ],
    },
];

// ---------------------------------------------------------------------------
// Navigation helper - mirrors getChildrenAtPath in tutorial.ts
// ---------------------------------------------------------------------------

/// Returns the children slice for `path_parts` within `nodes`, or `None` if not found.
fn get_children_at_path<'a>(
    nodes: &'a [Node],
    path_parts: &[&str],
) -> Option<&'a [Node]> {
    if path_parts.is_empty() {
        return Some(nodes);
    }
    let (head, rest) = (&path_parts[0], &path_parts[1..]);
    for node in nodes {
        if let Node::Branch { key, children } = node {
            // Path segments are the *stripped* display text of an Obj key (see
            // `navigate_right_raw`, which pushes `tags::strip_display(&o.key)`).
            // After i18n migration, `key` is a Fluent message ID like
            // `tutorial-branch-023`; we must translate it through the same
            // chain `node_to_ffon` uses to render the Obj key, then strip
            // display tags. Otherwise the path that the app records on
            // navigation can never resolve back to the branch.
            let translated = translate_node_string(key);
            if sicompass_sdk::tags::strip_display(&translated) == **head {
                return get_children_at_path(children, rest);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Convert static tree to FfonElement vec, substituting asset paths
// ---------------------------------------------------------------------------

fn node_to_ffon(node: &Node, texture_jpg: &str, ffon_json: &str) -> FfonElement {
    match node {
        Node::Leaf(s) => {
            // Resolve the translation key first, then run asset-placeholder
            // substitution on the resolved value. Asset sentinels like
            // `__TEXTURE_JPG__` live in the FTL value (or in non-key
            // literals such as `I_PLACEHOLDER`), so this order is required.
            let translated = translate_node_string(s);
            FfonElement::Str(apply_asset_placeholders(&translated, texture_jpg, ffon_json))
        }
        Node::Branch { key, children } => {
            let translated = translate_node_string(key);
            let resolved_key = apply_asset_placeholders(&translated, texture_jpg, ffon_json);
            let mut obj = FfonElement::new_obj(resolved_key);
            for child in *children {
                obj.as_obj_mut()
                    .unwrap()
                    .push(node_to_ffon(child, texture_jpg, ffon_json));
            }
            obj
        }
    }
}

/// Resolve a SECTIONS node string through the localizer. Strings that look
/// like Fluent message IDs (alphanumeric + hyphens, no whitespace or other
/// special chars) are routed through `t()`; everything else (the
/// `I_PLACEHOLDER` literal that the tutorial uses for an inline input slot)
/// is returned as-is.
fn translate_node_string(s: &str) -> String {
    register_translations();
    let looks_like_key = !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !looks_like_key {
        return s.to_owned();
    }
    let resolved = localize::t(s);
    if resolved == s {
        // Unknown key — leave the literal in place so a missed entry shows
        // up loudly rather than silently.
        s.to_owned()
    } else {
        resolved
    }
}

fn apply_asset_placeholders(s: &str, texture_jpg: &str, ffon_json: &str) -> String {
    match s {
        "__TEXTURE_JPG__" => format!("<image>{texture_jpg}</image>"),
        "__IMAGE_WITH_PREFIX_SUFFIX__" => {
            format!("Image with prefix: <image>{texture_jpg}</image>and suffix")
        }
        "__IMAGE_SUFFIX_ONLY__" => format!("<image>{texture_jpg}</image>and suffix"),
        "__IMAGE_PREFIX_ONLY__" => format!("Image with prefix: <image>{texture_jpg}</image>"),
        "__LINK_WITH_PREFIX_SUFFIX__" => {
            format!("Playground based on html <link>{ffon_json}</link> which can be with prefix and suffix")
        }
        "__LOREM_IPSUM__" => lorem_ipsum().to_owned(),
        other => other.to_owned(),
    }
}

fn nodes_to_ffon(nodes: &[Node], texture_jpg: &str, ffon_json: &str) -> Vec<FfonElement> {
    nodes.iter().map(|n| node_to_ffon(n, texture_jpg, ffon_json)).collect()
}

// ---------------------------------------------------------------------------
// TutorialProvider
// ---------------------------------------------------------------------------

/// The tutorial provider: a read-only navigable guide to Sicompass.
///
/// `assets_dir` should point to the directory containing `texture.jpg` and `sf.json`.
/// These are the same assets used by the TypeScript tutorial (`lib/lib_tutorial/assets/`).
pub struct TutorialProvider {
    current_path: String,
    texture_jpg: String,
    ffon_json: String,
}

impl TutorialProvider {
    /// Create with explicit asset directory.
    pub fn new(assets_dir: &Path) -> Self {
        let texture_jpg = assets_dir.join("texture.jpg").to_string_lossy().replace('\\', "/");
        let ffon_json = assets_dir.join("ffon.json").to_string_lossy().replace('\\', "/");
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg,
            ffon_json,
        }
    }

    /// Convenience: create with an empty asset path (for tests that don't need images/links).
    pub fn new_headless() -> Self {
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg: "/missing/texture.jpg".to_owned(),
            ffon_json: "/missing/ffon.json".to_owned(),
        }
    }

    fn path_parts(&self) -> Vec<&str> {
        if self.current_path == "/" {
            vec![]
        } else {
            self.current_path.split('/').filter(|s| !s.is_empty()).collect()
        }
    }
}

impl Provider for TutorialProvider {
    fn name(&self) -> &str { "tutorial" }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("tutorial-display-name")
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let parts = self.path_parts();
        match get_children_at_path(SECTIONS, &parts) {
            Some(nodes) => nodes_to_ffon(nodes, &self.texture_jpg, &self.ffon_json),
            None => vec![],
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }
}

// ---------------------------------------------------------------------------
// Tests - port of tests/lib_tutorial/tutorial.test.ts (12 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> TutorialProvider {
        TutorialProvider::new_headless()
    }

    // Root level

    #[test]
    fn test_root_returns_top_level_sections() {
        let mut p = provider();
        let elems = p.fetch();
        assert!(!elems.is_empty());
        // First section is "Welcome..."
        let first = elems[0].as_obj().unwrap();
        assert!(first.key.starts_with("Welcome"));
    }

    #[test]
    fn test_root_contains_all_top_level_sections() {
        let mut p = provider();
        let elems = p.fetch();
        let keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(keys.iter().any(|k| k.starts_with("Navigation")));
        assert!(keys.iter().any(|k| k.starts_with("Editing")));
        assert!(keys.iter().any(|k| k.starts_with("Commands")));
        assert!(keys.iter().any(|k| k.starts_with("Programs")));
        assert!(keys.iter().any(|k| k.starts_with("Interactive")));
        assert!(keys.iter().any(|k| k.starts_with("Development")));
        assert!(keys.iter().any(|k| k.starts_with("Next Steps")));
    }

    // Path navigation

    #[test]
    fn test_navigation_section_children() {
        let mut p = provider();
        p.push_path("Navigation");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        // First child is a string (overview text)
        assert!(elems[0].is_str());
        // Then "Moving Around" and "Modes" sections
        let section_keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(section_keys.contains(&"Moving Around"));
        assert!(section_keys.contains(&"Modes"));
    }

    #[test]
    fn test_nested_navigation() {
        let mut p = provider();
        p.push_path("Navigation");
        p.push_path("Modes");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        assert!(elems[0].is_str());
    }

    #[test]
    fn test_unknown_path_returns_empty() {
        let mut p = provider();
        p.push_path("NonExistentSection");
        let elems = p.fetch();
        assert!(elems.is_empty());
    }

    // Interactive elements

    #[test]
    fn test_interactive_elements_contains_checkbox() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_checked_checkbox = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.starts_with("<checkbox checked>"))
        });
        assert!(has_checked_checkbox);
    }

    #[test]
    fn test_interactive_elements_contains_radio_group() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_radio = elems.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.starts_with("<radio>"))
        });
        assert!(has_radio);
    }

    #[test]
    fn test_interactive_elements_contains_input() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_input = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<input>"))
        });
        assert!(has_input);
    }

    #[test]
    fn test_interactive_elements_contains_image() {
        let mut p = provider();
        p.push_path("Interactive Elements");
        let elems = p.fetch();
        let has_image = elems.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<image>"))
        });
        assert!(has_image);
    }

    #[test]
    fn test_input_example_path_round_trips() {
        // Navigating Right into the "+i input example <input></input>" Obj
        // pushes the *stripped* display key as the path segment. Restoring
        // that path (on tab-switch / app restart) must still resolve to the
        // Obj's I_PLACEHOLDER child, otherwise the saved cursor collapses out
        // of the Obj after a close/reopen cycle.
        let stripped =
            sicompass_sdk::tags::strip_display("+i input example <input></input>");
        let mut p = provider();
        p.push_path("Interactive Elements");
        p.push_path(&stripped);
        let elems = p.fetch();
        assert_eq!(elems.len(), 1, "expected the I_PLACEHOLDER child to resolve");
        assert!(
            elems[0].as_str().map_or(false, |s| s == I_PLACEHOLDER),
            "child should be the I_PLACEHOLDER leaf, got: {:?}",
            elems[0]
        );
    }

    // Pop path

    #[test]
    fn test_pop_path_returns_to_root() {
        let mut p = provider();
        p.push_path("Navigation");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        let elems = p.fetch();
        // Should return all top-level sections again
        assert!(elems.len() > 5);
    }

    #[test]
    fn test_pop_path_from_nested() {
        let mut p = provider();
        p.push_path("Navigation");
        p.push_path("Modes");
        p.pop_path();
        assert_eq!(p.current_path(), "/Navigation");
        let elems = p.fetch();
        // Should return Navigation's children
        let keys: Vec<&str> = elems.iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(keys.contains(&"Moving Around"));
    }

    // set_current_path

    #[test]
    fn test_set_current_path() {
        let mut p = provider();
        p.set_current_path("/Navigation/Modes");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        assert!(elems[0].is_str());
    }

    // Undo and Redo section

    #[test]
    fn test_undo_and_redo_section_present_at_root() {
        let mut p = provider();
        let elems = p.fetch();
        let keys: Vec<&str> = elems
            .iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(
            keys.iter().any(|k| *k == "Undo and Redo"),
            "Undo and Redo section must appear at the root, got: {:?}",
            keys
        );
    }

    #[test]
    fn test_undo_and_redo_section_has_subsections() {
        let mut p = provider();
        p.push_path("Undo and Redo");
        let elems = p.fetch();
        let section_keys: Vec<&str> = elems
            .iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(section_keys.contains(&"What gets recorded"));
        assert!(section_keys.contains(&"What cannot be undone"));
        assert!(section_keys.contains(&"Walking the path back"));
    }

    // Tabs section (added 2026-05; renamed from "Tabs and Windows" after the
    // Ctrl+N spawn-new-window shortcut was reverted — multi-window no longer
    // exists, so the section and its tests no longer mention windows/Ctrl+N).

    #[test]
    fn test_tabs_section_present_under_navigation() {
        let mut p = provider();
        p.push_path("Navigation");
        let elems = p.fetch();
        let section_keys: Vec<&str> = elems
            .iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(
            section_keys.contains(&"Tabs"),
            "Tabs must appear under Navigation, got: {:?}",
            section_keys
        );
    }

    #[test]
    fn test_tabs_section_mentions_core_shortcuts() {
        let mut p = provider();
        p.set_current_path("/Navigation/Tabs");
        let elems = p.fetch();
        let joined: String = elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n");
        for token in ["Ctrl+T", "Ctrl+W", "Ctrl+Tab"] {
            assert!(
                joined.contains(token),
                "Tabs section must mention {token}, content was:\n{joined}"
            );
        }
        // Ctrl+N (spawn new window) was reverted; it must no longer appear.
        assert!(
            !joined.contains("Ctrl+N"),
            "Tabs section must not mention the reverted Ctrl+N window shortcut, content was:\n{joined}"
        );
    }

    #[test]
    fn test_tabs_section_mentions_busy_close_confirmation() {
        let mut p = provider();
        p.set_current_path("/Navigation/Tabs");
        let elems = p.fetch();
        let joined: String = elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        assert!(
            joined.contains("busy") && joined.contains("confirmation"),
            "Tabs section must describe the busy-tab close confirmation, content was:\n{joined}"
        );
    }

    // Accessibility: per-item screen-reader language (added 2026-05)

    #[test]
    fn test_accessibility_section_mentions_per_item_language() {
        let mut p = provider();
        p.push_path("Accessibility");
        let elems = p.fetch();
        let joined: String = elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        assert!(
            joined.contains("language"),
            "Accessibility section must describe per-item language detection, content was:\n{joined}"
        );
    }

    // Editing: Enter appends only in editor providers (added 2026-06)

    #[test]
    fn test_editing_section_mentions_enter_append() {
        let mut p = provider();
        p.push_path("Editing");
        let elems = p.fetch();
        let joined: String = elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("Enter") && joined.to_lowercase().contains("append"),
            "Editing section must describe Enter-to-append in editor providers, content was:\n{joined}"
        );
    }

    // Updates section (added 2026-05)

    #[test]
    fn test_updates_section_present_under_configuration() {
        let mut p = provider();
        p.push_path("Configuration");
        let elems = p.fetch();
        let section_keys: Vec<&str> = elems
            .iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.as_str()))
            .collect();
        assert!(
            section_keys.contains(&"Updates"),
            "Updates must appear under Configuration, got: {:?}",
            section_keys
        );
    }

    #[test]
    fn test_updates_section_mentions_ctrl_u_and_signature_verification() {
        let mut p = provider();
        p.set_current_path("/Configuration/Updates");
        let elems = p.fetch();
        let joined: String = elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Ctrl+U"));
        assert!(joined.contains("SHA-256"));
        assert!(joined.contains("ed25519"));
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the tutorial with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("tutorial", || {
        let assets = sicompass_sdk::platform::resolve_repo_asset("lib/lib_tutorial/assets");
        Box::new(TutorialProvider::new(&assets))
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("tutorial", "tutorial").enable_by_default(),
    );
}
