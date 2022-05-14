use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::{env, fmt, fs, io};

use chrono::Utc;
use color_eyre::Help;
use dialoguer::{theme, Confirm, Input, Select};
use directories_next::{ProjectDirs, UserDirs};
use eyre::eyre;
use hypothesis::annotations::{Annotation, Document, Permissions, Selector, Target, UserInfo};
use hypothesis::{Hypothesis, UserAccountID};
use serde::{Deserialize, Serialize};

use crate::errors::Apologize;
use crate::gooseberry::knowledge_base::{
    get_handlebars, AnnotationTemplate, LinkTemplate, PageTemplate, Templates,
};
use crate::{utils, NAME};

pub static DEFAULT_NESTED_TAG: &str = "/";
pub static DEFAULT_ANNOTATION_TEMPLATE: &str = r#"

### {{id}}
Created: {{date_format "%c" created}}
Tags: {{#each tags}}{{this}}{{#unless @last}}, {{/unless}}{{/each}}

{{#each highlight}}> {{this}}{{/each}}

{{text}}

[See in context]({{incontext}}) at [{{title}}]({{uri}})

"#;
pub static DEFAULT_PAGE_TEMPLATE: &str = r#"
# {{name}}
{{#each annotations}}{{this}}{{/each}}

"#;
pub static DEFAULT_INDEX_LINK_TEMPLATE: &str = r#"
- [{{name}}]({{relative_path}})"#;
pub static DEFAULT_INDEX_FILENAME: &str = "SUMMARY";
pub static DEFAULT_FILE_EXTENSION: &str = "md";

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum OrderBy {
    Tag,
    URI,
    BaseURI,
    Title,
    ID,
    Empty,
    Created,
    Updated,
}

impl fmt::Display for OrderBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderBy::Tag => write!(f, "tag"),
            OrderBy::URI => write!(f, "uri"),
            OrderBy::BaseURI => write!(f, "base_uri"),
            OrderBy::Title => write!(f, "title"),
            OrderBy::ID => write!(f, "id"),
            OrderBy::Empty => write!(f, "empty"),
            OrderBy::Created => write!(f, "created"),
            OrderBy::Updated => write!(f, "updated"),
        }
    }
}

/// Configuration struct, asks for user input to fill in the optional values the first time gooseberry is run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GooseberryConfig {
    /// Hypothesis username
    pub(crate) hypothesis_username: Option<String>,
    /// Hypothesis personal API key
    pub(crate) hypothesis_key: Option<String>,
    /// Hypothesis group with knowledge base annotations
    pub(crate) hypothesis_group: Option<String>,

    /// Related to tagging and editing
    /// Directory to store `sled` database files
    pub(crate) db_dir: PathBuf,

    /// Relating to the generated markdown knowledge base:
    /// Directory to write out knowledge base markdown files
    pub(crate) kb_dir: Option<PathBuf>,
    /// Handlebars annotation template
    pub(crate) annotation_template: Option<String>,
    /// Handlebars index link template
    pub(crate) index_link_template: Option<String>,
    /// Handlebars page template
    pub(crate) page_template: Option<String>,
    /// Handlebars index file name
    pub(crate) index_name: Option<String>,
    /// Wiki file extension
    pub(crate) file_extension: Option<String>,
    /// Define the hierarchy of folders
    pub(crate) hierarchy: Option<Vec<OrderBy>>,
    /// Define how annotations on a page are sorted
    pub(crate) sort: Option<Vec<OrderBy>>,
    /// Define tags to ignore
    pub(crate) ignore_tags: Option<Vec<String>>,
    /// Define nested tag pattern
    pub(crate) nested_tag: Option<String>,
}

/// Main project directory, cross-platform
pub fn get_project_dir() -> color_eyre::Result<ProjectDirs> {
    Ok(ProjectDirs::from("rs", "", NAME).ok_or(Apologize::Homeless)?)
}

impl Default for GooseberryConfig {
    fn default() -> Self {
        let config = Self {
            hypothesis_username: None,
            hypothesis_key: None,
            hypothesis_group: None,
            db_dir: get_project_dir()
                .map(|dir| dir.data_dir().join("gooseberry_db"))
                .expect("Couldn't make database directory"),
            kb_dir: None,
            annotation_template: None,
            page_template: None,
            index_link_template: None,
            index_name: None,
            file_extension: None,
            hierarchy: None,
            sort: None,
            ignore_tags: None,
            nested_tag: None,
        };
        config.make_dirs().expect("Couldn't make directories");
        config
    }
}

impl GooseberryConfig {
    pub fn default_config(file: Option<&Path>) -> color_eyre::Result<()> {
        let writer: Box<dyn io::Write> = match file {
            Some(file) => Box::new(fs::File::create(file)?),
            None => Box::new(io::stdout()),
        };
        let mut buffered = io::BufWriter::new(writer);
        let contents = format!(
            r#"
hypothesis_username = '<Hypothesis username>'
hypothesis_key = '<Hypothesis personal API key>'
hypothesis_group = '<Hypothesis group ID to take annotations from>'
db_dir = '<full path to database folder>'
kb_dir = '<knowledge-base folder>'
hierarchy = ['Tag']
sort = ['Created']
ignore_tags = []
nested_tag = {}
annotation_template = '''{}'''
page_template = '''{}'''
index_link_template = '''{}'''
index_name = '{}'
file_extension = '{}'
"#,
            DEFAULT_NESTED_TAG,
            DEFAULT_ANNOTATION_TEMPLATE,
            DEFAULT_PAGE_TEMPLATE,
            DEFAULT_INDEX_LINK_TEMPLATE,
            DEFAULT_INDEX_FILENAME,
            DEFAULT_FILE_EXTENSION
        );
        write!(&mut buffered, "{}", contents)?;
        Ok(())
    }

    /// Print location of config.toml file
    pub fn print_location(config_file: Option<&Path>) -> color_eyre::Result<()> {
        println!("{}", Self::location(config_file)?.to_string_lossy());
        Ok(())
    }

    /// Make db and kb directories
    pub fn make_dirs(&self) -> color_eyre::Result<()> {
        if !self.db_dir.exists() {
            fs::create_dir_all(&self.db_dir).map_err(|e: io::Error| Apologize::ConfigError {
                message: format!(
                    "Couldn't create database directory {:?}, {}",
                    self.db_dir, e
                ),
            })?;
        }
        if let Some(kb_dir) = &self.kb_dir {
            if !kb_dir.exists() {
                fs::create_dir_all(&kb_dir).map_err(|e: io::Error| Apologize::ConfigError {
                    message: format!(
                        "Couldn't create knowledge base directory {:?}, {}",
                        kb_dir, e
                    ),
                })?;
            }
        }
        Ok(())
    }

    /// Get a template for making a custom config file
    /// If you leave kb_dir and hypothesis details empty, Gooseberry asks you for them the first time
    fn get_default_config_file() -> color_eyre::Result<PathBuf> {
        let dir = get_project_dir()?;
        let config_dir = dir.config_dir();
        Ok(config_dir.join(format!("{}.toml", NAME)))
    }

    /// Gets the current config file location
    pub fn location(config_file: Option<&Path>) -> color_eyre::Result<PathBuf> {
        match config_file {
            Some(path) => {
                if path.exists() {
                    Ok(PathBuf::from(path))
                } else {
                    let error: color_eyre::Result<PathBuf> = Err(Apologize::ConfigError {
                        message: format!("No such file {:?}", path),
                    }
                    .into());
                    error.suggestion(format!(
                        "Use `gooseberry config default {:?}` to write out the default configuration and modify the generated file",
                        path
                    ))
                }
            }
            None => Self::get_default_config_file(),
        }
    }

    /// Get current configuration
    /// Hides the developer key (except last three digits)
    pub fn get(config_file: Option<&Path>) -> color_eyre::Result<String> {
        let mut file = fs::File::open(Self::location(config_file)?)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents
            .split('\n')
            .map(|k| {
                let parts = k.split(" = ").collect::<Vec<_>>();
                if parts[0] == "hypothesis_key" {
                    format!(
                        "{} = '{}{}'\n",
                        parts[0],
                        (0..(parts[1].len() - 2 - 3))
                            .map(|_| '*')
                            .collect::<String>(),
                        &parts[1][parts[1].len() - 5..parts[1].len() - 2]
                    )
                } else {
                    format!("{}\n", parts.join(" = "))
                }
            })
            .collect::<String>())
    }

    /// Read config from default location
    pub async fn load(config_file: Option<&Path>) -> color_eyre::Result<Self> {
        // Reads the GOOSEBERRY_CONFIG environment variable to get config file location
        let mut config = match config_file {
            Some(path) => {
                if path.exists() {
                    let config: Self = confy::load_path(path)?;
                    config.make_dirs()?;
                    Ok(config)
                } else {
                    let error: color_eyre::Result<Self> = Err(Apologize::ConfigError {
                        message: format!("No such file {:?}", path),
                    }
                        .into());
                    error.suggestion(format!(
                        "Use `gooseberry config default {:?}` to write out the default configuration and modify the generated file",
                        path
                    ))
                }
            }
            None => {
                Ok(confy::load(NAME).suggestion(Apologize::ConfigError {
                    message: "Couldn't load from the default config location, maybe you don't have access? \
                    Try running `gooseberry config default config_file.toml`, modify the generated file, \
                then `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>`".into()
                })?)
            },
        }?;

        if config.hypothesis_username.is_none()
            || config.hypothesis_key.is_none()
            || !Self::authorize(
                config
                    .hypothesis_username
                    .as_deref()
                    .ok_or_else(|| eyre!("No hypothesis username"))?,
                config
                    .hypothesis_key
                    .as_deref()
                    .ok_or_else(|| eyre!("No hypothesis key"))?,
            )
            .await?
        {
            config.set_credentials().await?;
        }

        if config.hypothesis_group.is_none() {
            config.set_group().await?;
        }
        Ok(config)
    }

    /// Queries and sets all knowledge base related configuration options
    pub fn set_kb_all(&mut self) -> color_eyre::Result<()> {
        self.set_kb_dir()?;
        self.set_annotation_template()?;
        self.set_page_template()?;
        self.set_index_link_template()?;
        self.set_index_name()?;
        self.set_nested_tag()?;
        self.set_file_extension()?;
        self.set_hierarchy()?;
        self.set_sort()?;
        Ok(())
    }

    /// Sets the knowledge base directory
    pub fn set_kb_dir(&mut self) -> color_eyre::Result<()> {
        let default = UserDirs::new()
            .ok_or(Apologize::Homeless)?
            .home_dir()
            .join(crate::NAME);
        self.kb_dir = loop {
            println!("NOTE: the directory will be deleted and regenerated on each make!");
            let input = utils::user_input(
                "Directory to build knowledge base",
                Some(
                    default
                        .to_str()
                        .ok_or_else(|| eyre!("Couldn't convert directory to string"))?,
                ),
                true,
                false,
            )?;
            let path = Path::new(&input);
            if path.exists() || fs::create_dir(path).is_ok() {
                break Some(path.to_owned());
            } else {
                println!(
                    "\nDirectory could not be created, make sure all parent folders exist and you have the right permissions.\n"
                )
            }
        };
        self.store()?;
        Ok(())
    }

    fn get_order_bys(selections: Vec<OrderBy>) -> color_eyre::Result<Vec<OrderBy>> {
        let mut selections = selections;
        let selection = Select::with_theme(&theme::ColorfulTheme::default())
            .with_prompt("Field 1")
            .items(&selections[..])
            .interact()?;
        let mut order = Vec::new();
        if selections[selection] != OrderBy::Empty {
            order.push(selections[selection]);
            selections.remove(selection);
            selections.retain(|&x| x != OrderBy::Empty);
            let mut number = 2;
            loop {
                if selections.is_empty() {
                    break;
                }
                if Confirm::with_theme(&theme::ColorfulTheme::default())
                    .with_prompt("Add more fields?")
                    .interact()?
                {
                    let selection = Select::with_theme(&theme::ColorfulTheme::default())
                        .with_prompt(&format!("Field {}", number))
                        .items(&selections[..])
                        .interact()?;
                    order.push(selections[selection]);
                    selections.remove(selection);
                    number += 1
                } else {
                    break;
                }
            }
        }
        Ok(order)
    }

    /// Sets the hierarchy fields which determines the folder hierarchy
    pub fn set_hierarchy(&mut self) -> color_eyre::Result<()> {
        println!("Set folder hierarchy order");
        let selections = vec![
            OrderBy::Empty,
            OrderBy::Tag,
            OrderBy::URI,
            OrderBy::BaseURI,
            OrderBy::Title,
            OrderBy::ID,
        ];
        let order = Self::get_order_bys(selections)?;
        if order.is_empty() {
            println!(
                "Single file: {}.{}",
                self.index_name
                    .as_ref()
                    .ok_or_else(|| eyre!("No index name"))?,
                self.file_extension
                    .as_ref()
                    .ok_or_else(|| eyre!("No file extension"))?
            );
        } else {
            println!(
                "Folder structure: {}.{}",
                order
                    .iter()
                    .map(|o| o.to_string())
                    .collect::<Vec<_>>()
                    .join("/"),
                self.file_extension
                    .as_ref()
                    .ok_or_else(|| eyre!("No file extension"))?
            );
        }
        self.hierarchy = Some(order);
        self.store()?;
        Ok(())
    }

    /// Sets the sort order for annotations within a page
    pub fn set_sort(&mut self) -> color_eyre::Result<()> {
        println!("Set sort order for annotations within a page");
        let selections = vec![
            OrderBy::Tag,
            OrderBy::URI,
            OrderBy::BaseURI,
            OrderBy::ID,
            OrderBy::Title,
            OrderBy::Created,
            OrderBy::Updated,
        ];
        let order = Self::get_order_bys(selections)?;

        println!(
            "Sort order: {}",
            order
                .iter()
                .map(|o| o.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );

        self.sort = Some(order);
        self.store()?;
        Ok(())
    }

    pub fn set_ignore_tags(&mut self) -> color_eyre::Result<()> {
        println!("Set tags to ignore during knowledge base generation");
        let ignore_tags: String = Input::with_theme(&theme::ColorfulTheme::default())
            .with_prompt("Enter comma-separated tags")
            .with_initial_text(
                self.ignore_tags
                    .as_ref()
                    .map(|tags| tags.join(", "))
                    .unwrap_or_default(),
            )
            .allow_empty(true)
            .interact_text()?;
        if ignore_tags.is_empty() {
            self.ignore_tags = None
        } else {
            self.ignore_tags = Some(
                ignore_tags
                    .split(',')
                    .map(|t| t.trim().to_owned())
                    .collect(),
            )
        }
        self.store()?;
        Ok(())
    }

    pub(crate) fn get_templates(&self) -> Templates {
        Templates {
            annotation_template: self
                .annotation_template
                .as_deref()
                .unwrap_or(DEFAULT_ANNOTATION_TEMPLATE),
            page_template: self
                .page_template
                .as_deref()
                .unwrap_or(DEFAULT_PAGE_TEMPLATE),
            index_link_template: self
                .index_link_template
                .as_deref()
                .unwrap_or(DEFAULT_INDEX_LINK_TEMPLATE),
        }
    }
    /// Sets the annotation template in Handlebars format.
    pub fn set_annotation_template(&mut self) -> color_eyre::Result<()> {
        let selections = &[
            "Use default annotation template",
            "Edit annotation template",
        ];

        let selection = Select::with_theme(&theme::ColorfulTheme::default())
            .with_prompt("How should gooseberry format annotations?")
            .items(&selections[..])
            .interact()?;
        if selection == 0 {
            self.annotation_template = Some(DEFAULT_ANNOTATION_TEMPLATE.to_string());
        } else {
            let test_annotation = Annotation {
                id: "test".to_string(),
                created: Utc::now(),
                updated: Utc::now(),
                user: Default::default(),
                uri: "https://github.com/out-of-cheese-error/gooseberry".to_string(),
                text: "testing annotation".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                group: "group_id".to_string(),
                permissions: Permissions {
                    read: vec![],
                    delete: vec![],
                    admin: vec![],
                    update: vec![],
                },
                target: vec![Target::builder()
                    .source("https://www.example.com")
                    .selector(vec![Selector::new_quote(
                        "exact text in website to highlight",
                        "prefix of text",
                        "suffix of text",
                    )])
                    .build()?],
                links: vec![(
                    "incontext".to_string(),
                    "https://incontext_link.com".to_string(),
                )]
                .into_iter()
                .collect(),
                hidden: false,
                flagged: false,
                document: Some(Document {
                    title: vec!["Web page title".into()],
                    dc: None,
                    highwire: None,
                    link: vec![],
                }),
                references: vec![],
                user_info: Some(UserInfo {
                    display_name: Some("test_display_name".to_string()),
                }),
            };
            let test_markdown_annotation = AnnotationTemplate::from_annotation(test_annotation);
            self.annotation_template = loop {
                let template = utils::external_editor_input(
                    Some(
                        self.annotation_template
                            .as_deref()
                            .unwrap_or(DEFAULT_ANNOTATION_TEMPLATE),
                    ),
                    ".hbs",
                )?;
                let templates = Templates {
                    annotation_template: &template,
                    ..Default::default()
                };
                match get_handlebars(templates)
                    .map(|hbs| hbs.render("annotation", &test_markdown_annotation))
                {
                    Err(e) => {
                        eprintln!("TemplateRenderError: {}\n Try again.", e);
                        continue;
                    }
                    Ok(Err(e)) => {
                        eprintln!("TemplateRenderError: {}\n Try again.", e);
                        continue;
                    }
                    Ok(Ok(md)) => {
                        println!("Template looks like this:");
                        println!();
                        println!("{}", md)
                    }
                }
                break Some(template);
            };
        }
        self.store()?;
        Ok(())
    }

    /// Sets the annotation template in Handlebars format.
    pub fn set_page_template(&mut self) -> color_eyre::Result<()> {
        let selections = &["Use default page template", "Edit page template"];

        let selection = Select::with_theme(&theme::ColorfulTheme::default())
            .with_prompt("How should gooseberry format pages?")
            .items(&selections[..])
            .interact()?;
        if selection == 0 {
            self.page_template = Some(DEFAULT_PAGE_TEMPLATE.to_string());
        } else {
            let test_annotation_1 = Annotation {
                id: "test".to_string(),
                created: Utc::now(),
                updated: Utc::now(),
                user: Default::default(),
                uri: "https://github.com/out-of-cheese-error/gooseberry".to_string(),
                text: "testing annotation".to_string(),
                tags: vec!["tag1".to_string(), "tag2".to_string()],
                group: "group_id".to_string(),
                permissions: Permissions {
                    read: vec![],
                    delete: vec![],
                    admin: vec![],
                    update: vec![],
                },
                target: vec![Target::builder()
                    .source("https://www.example.com")
                    .selector(vec![Selector::new_quote(
                        "exact text in website to highlight\nmore text",
                        "prefix of text",
                        "suffix of text",
                    )])
                    .build()?],
                links: vec![(
                    "incontext".to_string(),
                    "https://incontext_link.com".to_string(),
                )]
                .into_iter()
                .collect(),
                hidden: false,
                flagged: false,
                document: Some(Document {
                    title: vec!["Web page title".into()],
                    dc: None,
                    highwire: None,
                    link: vec![],
                }),
                references: vec![],
                user_info: Some(UserInfo {
                    display_name: Some("test_display_name".to_string()),
                }),
            };
            let mut test_annotation_2 = test_annotation_1.clone();
            test_annotation_2.text = "Another annotation".to_string();

            let templates = Templates {
                annotation_template: self
                    .annotation_template
                    .as_ref()
                    .ok_or_else(|| eyre!("No annotation template"))?,
                ..Default::default()
            };
            let hbs = get_handlebars(templates)?;

            let page_data = PageTemplate {
                link_data: LinkTemplate {
                    name: "page_name".to_string(),
                    relative_path: "relative/path/to/page.md".to_string(),
                    absolute_path: "absolute/path/to/page.md".to_string(),
                },
                annotations: vec![test_annotation_1.clone(), test_annotation_2.clone()]
                    .into_iter()
                    .map(|a| hbs.render("annotation", &AnnotationTemplate::from_annotation(a)))
                    .collect::<Result<Vec<String>, _>>()?,
                raw_annotations: vec![
                    AnnotationTemplate::from_annotation(test_annotation_1),
                    AnnotationTemplate::from_annotation(test_annotation_2),
                ],
            };

            self.page_template = loop {
                let template = utils::external_editor_input(
                    Some(
                        self.page_template
                            .as_deref()
                            .unwrap_or(DEFAULT_PAGE_TEMPLATE),
                    ),
                    ".hbs",
                )?;
                let templates = Templates {
                    page_template: &template,
                    ..Default::default()
                };
                match get_handlebars(templates).map(|hbs| hbs.render("page", &page_data)) {
                    Err(e) => {
                        eprintln!("TemplateRenderError: {}\n Try again.", e);
                        continue;
                    }
                    Ok(Err(e)) => {
                        eprintln!("TemplateRenderError: {}\n Try again.", e);
                        continue;
                    }
                    Ok(Ok(md)) => {
                        println!("Template looks like this:");
                        println!();
                        println!("{}", md)
                    }
                }
                break Some(template);
            };
        }
        self.store()?;
        Ok(())
    }

    /// Sets the annotation template in Handlebars format.
    pub fn set_index_link_template(&mut self) -> color_eyre::Result<()> {
        let selections = &[
            "Use default index link template",
            "Edit index link template",
        ];

        let selection = Select::with_theme(&theme::ColorfulTheme::default())
            .with_prompt("How should gooseberry format the link in the Index file?")
            .items(&selections[..])
            .interact()?;
        if selection == 0 {
            self.index_link_template = Some(DEFAULT_INDEX_LINK_TEMPLATE.to_string());
        } else {
            self.index_link_template = loop {
                let template = utils::external_editor_input(
                    Some(
                        self.index_link_template
                            .as_deref()
                            .unwrap_or(DEFAULT_INDEX_LINK_TEMPLATE),
                    ),
                    ".hbs",
                )?;
                let templates = Templates {
                    index_link_template: &template,
                    ..Default::default()
                };
                if let Err(e) = get_handlebars(templates) {
                    eprintln!("TemplateRenderError: {}\n Try again.", e);
                    continue;
                }
                break Some(template);
            };
        }
        self.store()?;
        Ok(())
    }

    pub fn set_index_name(&mut self) -> color_eyre::Result<()> {
        self.index_name = Some(utils::user_input(
            "What name should gooseberry use for the index file",
            Some(self.index_name.as_deref().unwrap_or(DEFAULT_INDEX_FILENAME)),
            true,
            false,
        )?);
        self.store()?;
        Ok(())
    }

    pub fn set_nested_tag(&mut self) -> color_eyre::Result<()> {
        self.nested_tag = Some(utils::user_input(
            "What pattern should gooseberry use to define nested tags",
            Some(self.nested_tag.as_deref().unwrap_or(DEFAULT_NESTED_TAG)),
            true,
            false,
        )?);
        self.store()?;
        Ok(())
    }

    pub fn set_file_extension(&mut self) -> color_eyre::Result<()> {
        self.file_extension = Some(utils::user_input(
            "What extension should gooseberry use for wiki files",
            Some(
                self.file_extension
                    .as_deref()
                    .unwrap_or(DEFAULT_FILE_EXTENSION),
            ),
            true,
            false,
        )?);
        self.store()?;
        Ok(())
    }

    /// Sets the Hypothesis group used for Gooseberry annotations
    /// This opens a command-line prompt wherein the user can select creating a new group or
    /// using an existing group by ID
    pub async fn set_group(&mut self) -> color_eyre::Result<()> {
        let selections = &[
            "Create a new Hypothesis group",
            "Use an existing Hypothesis group",
        ];

        let group_id = loop {
            let selection = Select::with_theme(&theme::ColorfulTheme::default())
                .with_prompt("Where should gooseberry take annotations from?")
                .items(&selections[..])
                .interact()?;

            let (username, key) = (
                self.hypothesis_username
                    .as_deref()
                    .ok_or_else(|| eyre!("No Hypothesis username"))?,
                self.hypothesis_key
                    .as_deref()
                    .ok_or_else(|| eyre!("No Hypothesis key"))?,
            );
            if selection == 0 {
                let group_name = utils::user_input("Enter a group name", Some(NAME), true, false)?;
                let group_id = Hypothesis::new(username, key)?
                    .create_group(&group_name, Some("Gooseberry knowledge base annotations"))
                    .await?
                    .id;
                break group_id;
            } else {
                let api = Hypothesis::new(username, key)?;
                let groups = api
                    .get_groups(&hypothesis::groups::GroupFilters::default())
                    .await?;
                let group_selection: Vec<_> = groups
                    .iter()
                    .map(|g| format!("{}: {}", g.id, g.name))
                    .collect();
                let group_index = Select::with_theme(&theme::ColorfulTheme::default())
                    .with_prompt("Which group should gooseberry use?")
                    .items(&group_selection[..])
                    .interact()?;
                let group_id = groups[group_index].id.to_owned();
                if api.fetch_group(&group_id, Vec::new()).await.is_ok() {
                    break group_id;
                } else {
                    println!(
                        "\nGroup could not be loaded, please try again.\n\
                          Make sure the group exists and you are authorized to access it.\n\n"
                    )
                }
            }
        };

        self.hypothesis_group = Some(group_id);
        self.store()?;
        Ok(())
    }

    /// Check if user can be authorized
    pub async fn authorize(name: &str, key: &str) -> color_eyre::Result<bool> {
        Ok(Hypothesis::new(name, key)?
            .fetch_user_profile()
            .await?
            .userid
            == Some(UserAccountID(format!("acct:{}@hypothes.is", name))))
    }

    /// Asks user for Hypothesis credentials and sets them in the config
    pub async fn request_credentials(&mut self) -> color_eyre::Result<()> {
        let mut name = String::new();
        let mut key;
        loop {
            name = utils::user_input(
                "Hypothesis username",
                if name.is_empty() { None } else { Some(&name) },
                true,
                false,
            )?;
            key = dialoguer::Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Hypothesis developer API key")
                .interact()?;
            if Self::authorize(&name, &key).await? {
                self.hypothesis_username = Some(name);
                self.hypothesis_key = Some(key);
                self.store()?;
                return Ok(());
            } else {
                println!("Could not authorize your Hypothesis credentials, please try again.");
            }
        }
    }
    /// Reads the `HYPOTHESIS_NAME` and `HYPOTHESIS_KEY` environment variables to get Hypothesis credentials.
    /// If not present or invalid, requests credentials from user.
    pub async fn set_credentials(&mut self) -> color_eyre::Result<()> {
        let (name, key) = (
            env::var("HYPOTHESIS_NAME").ok(),
            env::var("HYPOTHESIS_KEY").ok(),
        );
        if let (Some(n), Some(k)) = (&name, &key) {
            if Self::authorize(n, k).await? {
                self.hypothesis_username = Some(n.to_owned());
                self.hypothesis_key = Some(k.to_owned());
                self.store()?;
            } else {
                println!(
                    "Authorization with environment variables did not work. Enter details below"
                );
                self.request_credentials().await?;
            }
        } else {
            self.request_credentials().await?;
        }
        Ok(())
    }

    /// Write possibly modified config
    pub fn store(&self) -> color_eyre::Result<()> {
        // Reads the GOOSEBERRY_CONFIG environment variable to get config file location
        let config_file = env::var("GOOSEBERRY_CONFIG").ok();
        match config_file {
            Some(file) => confy::store_path(Path::new(&file), &(*self).clone()).suggestion(Apologize::ConfigError {
                message: "The current config_file location does not seem to have write access. \
                   Use `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>` to set a new location".into()
            })?,
            None => confy::store(NAME, &(*self).clone()).suggestion(Apologize::ConfigError {
                message: "The current config_file location does not seem to have write access. \
                    Use `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>` to set a new location".into()
            })?,
        };
        Ok(())
    }
}
