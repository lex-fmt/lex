lex.ing Site - Architecture

This directory contains the source for lex.ing, the documentation site for
the lex markup language. The site uses Jekyll for templating and GitHub Pages
for hosting, with page content authored in lex format.


1. Directory Structure

   docs/
   ├── _config.yml          Jekyll configuration
   ├── Gemfile              Ruby dependencies
   ├── CNAME                Custom domain (lex.ing)
   ├── .ruby-version        Ruby version for rbenv
   │
   ├── _layouts/
   │   └── default.html     Site template (header, nav, footer)
   │
   ├── _includes/
   │   └── _lex/            Generated HTML fragments (gitignored)
   │       ├── index.html
   │       ├── about.html
   │       └── ...
   │
   ├── css/                 Stylesheets
   │   ├── variables.css    Shared design tokens (fonts, colors, spacing)
   │   ├── site.css         Site chrome (header, nav, footer, markdown)
   │   └── lex-content.css  Lex document rendering (.lex-* classes)
   │
   ├── content/             Lex source files (what you edit)
   │   ├── index.lex
   │   ├── about.lex
   │   ├── why.lex
   │   ├── tools.lex
   │   └── dummy-session.lex
   │
   ├── index.md             Jekyll page wrappers (thin, rarely change)
   ├── about.md             Each just includes the generated HTML:
   ├── why.md                 {% include _lex/about.html %}
   ├── tools.md
   ├── dummy-session.md
   ├── editors.md           Pure markdown pages (no lex source)
   ├── contributing.md
   │
   ├── build                Lex → HTML build script
   ├── serve                Local dev server (Jekyll + livereload)
   │
   └── _site/               Jekyll output (gitignored)


2. Build Pipeline

   The site generation has two stages:

   1. Lex → HTML (./build)
      For each content/*.lex file:
      - Run: lex <file> --to html --extras-css-path css/lex-content.css
      - Extract the <div class="lex-document">...</div> fragment
      - Write to _includes/_lex/<name>.html
   2. Jekyll Build
      Jekyll processes the site:
      - Page wrappers (*.md) include the generated HTML fragments
      - Layouts wrap content with site chrome (header, nav, footer)
      - Output goes to _site/


3, Local Development

   Prerequisites:
   - Ruby 3.2+ (via rbenv)
   - Bundler
   - lex CLI (from lex-fmt/tools releases or built locally)
   - direnv (optional, for automatic environment setup)

   Setup:
      cd docs
      bundle install
      direnv allow               # Sets LEX_BIN from .envrc
   Build lex content:
      ./build                    # Uses LEX_BIN from .envrc or 'lex' from PATH
   Run dev server:
      ../serve                   # http://localhost:4000 with livereload
      ../serve --port 8080       # Custom port
   :: shell :: 


4, GitHub Pages Deployment

   The site deploys automatically on push to main via GitHub Actions.

   Workflow (.github/workflows/pages.yml):

   1. Checkout repository
   2. Setup Ruby + bundle install
   3. Download lex CLI from lex-fmt/tools releases
   4. Run ./build (generates _includes/_lex/*.html)
   5. Run Jekyll build
   6. Deploy to GitHub Pages

   The lex CLI version is pinned in the workflow (LEX_VERSION).


5. CSS Architecture

   Three files, loaded in order:
   
   1. variables.css
      - CSS custom properties for fonts, colors, spacing
      - Change typography/colors here to update entire site
   2. site.css
      - Site chrome: header, navigation, footer
      - Markdown content styling (for non-lex pages like editors.md)
      - Scoped to site structure elements
   3. lex-content.css
      - Lex document rendering
      - All rules scoped to .lex-* classes
      - Can be used standalone for lex HTML exports


6. Adding a New Page

   For a lex-authored page:

   1. Create content/<name>.lex with your content
   2. Create <name>.md wrapper:
         ---
         layout: default
         title: Page Title
         ---
         {% include _lex/<name>.html %}
      :: md ::
   3. Run ./build to generate the HTML
   4. Add nav link in _layouts/default.html if needed

   For a pure markdown page:

   1. Create <name>.md with front matter and content
   2. Add nav link if needed


7. Updating Lex CLI Version

   Edit .github/workflows/pages.yml:

      env:
         LEX_VERSION: lex-cli-v0.2.6    # Update this
         LEX_ARCHIVE: lex-x86_64-unknown-linux-gnu.tar.gz
   :: yaml :: 

   Releases are at: https://github.com/lex-fmt/tools/releases
