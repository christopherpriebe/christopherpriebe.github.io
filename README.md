# Christopher Priebe's Personal Website

## Page Front Matter Guide

The following information is applicable to pages created under the `pages` collection (i.e., in the `_pages` folder).
There is a short section on caveats to this at the end.

### Base

The `base` layout includes the boilerplate for each HTML file: the head, the navigation row (on every page but the home page), and the footer.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation row.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation row. Required if `nav` is `true`.      | Conditional          | Number |
| `nav_title` | A shorter label for the navigation row (e.g., "Food" for "Food & drink").      | No (default: `title`)| String |

### Home

The `home` layout is the index page: the name, a lede, the section index, the education timeline (from `_history`), and a contact line.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `lede`      | The sentence under the name.                                                   | Yes                  | String |
| `index`     | The rows of the section index. Each takes a `title` and `url`, plus either a `note` or a `count` (`publications`, `projects`, `posts`, `places`, or `routes`) that the layout works out from the site. | Yes | List |

### Single Column Page

The `single_column_page` layout is a general-purpose layout for a variety of pages that is... a single column. Its content is styled as prose.

| Variable      | Description                                                                    | Required             | Type   |
|---------------|--------------------------------------------------------------------------------|----------------------|--------|
| `show_title`  | Set to `true` if the title should be shown as a header at the top of the page. | No (default: `false`)| Boolean|
| `description` | Shown as a lede under the title when `show_title` is `true`.                   | No                   | String |

### About

The `about` layout sets the page's Markdown as prose, with the first paragraph at lede size, followed by an email and CV line.

### Projects

The `projects` layout lists everything in the `projects` collection, ordered by `importance`.
Each project takes `description`, `years`, `venue`, `status`, `website`, `github`, `paper`, `links`, and `categories`.
Set `featured: true` to lead the list with a project: it takes a square mark and a "Featured" note, its description is set as a lede, and it shows its `summary` (Markdown) and `talks`.
A category filter appears once there is more than one project and more than one category.

### Instruments

The `instruments` layout holds the interactive instruments and a list of planned ones.

| Variable       | Description                                                                   | Required             | Type   |
|----------------|-------------------------------------------------------------------------------|----------------------|--------|
| `lede`         | The sentence under the title.                                                 | Yes                  | String |
| `planned`      | Groups of planned work, each with a `group` name and `items` (`title`, `status`, `description`). | No | List |
| `planned_note` | A closing line under the last group.                                          | No                   | String |

### Resume

The `resume` layout is specifically for a resume/CV page, built from the `resume` collection and the `resume_*` settings in `_config.yml`.

### Caveats

- All pages created under the `projects` collection (i.e., in the `_projects` folder) are defaulted to have `layout: single_column_page` and `show_title: true`.

## Design Notes

- The palette lives in `_sass/skins/default.sass`. Five neutrals carry every page; the signal colours are fills and strokes only, used inside the instrument plane and on the maps, never for type.
- Spacing is set on a 9px module (18, 27, 36 … 180), and the page is anchored to the left margin rather than centred. The plumb line in `_sass/_layout.sass` is measured from that margin.
- Type is Archivo for display, Public Sans for text, and IBM Plex Mono for labels, loaded from Google Fonts in `_includes/head.liquid`.
- There are three components: an entry (meta line, title, paragraph), a ruled row, and a segmented control. See `_sass/_entries.sass`.
- Publication venues link through `_data/venues.yaml`, keyed by the bibliography's `abbr`: to that year's edition under `years` if listed, else to the venue's `url`.
- Blog posts are published as an Atom feed at `/feed.xml` by jekyll-feed; a post's `description` is its summary there and in the archive.
- Photos stay sharp rectangles set on a flat concrete plane offset 18px down and to the right, with no border, radius or shadow (`_sass/_photos.sass`). From any page or post:
  `{% include photo.liquid src="/assets/images/x.jpg" alt="…" caption="…" shape="landscape" %}`.
  `shape` is `portrait` (3:4), `landscape` (3:2), or `square`, or leave it out to keep the image's own ratio; `focus` sets what a crop keeps (e.g. `"50% 20%"`).
