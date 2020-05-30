use crate::gooseberry::Gooseberry;
use mdbook::config::Config;
use mdbook::MDBook;

impl Gooseberry {
    pub fn make(&self) -> color_eyre::Result<()> {
        // create a default config and change a couple things
        let mut cfg = Config::default();
        cfg.book.title = Some("Gooseberry".to_string());
        cfg.book.authors.push(self.api.username.to_string());

        let mut book = MDBook::init(&self.config.kb_dir)
            .create_gitignore(true)
            .with_config(cfg)
            .build()?;

        Ok(())
    }
}
