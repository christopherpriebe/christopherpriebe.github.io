# Christopher Priebe's Personal Website

## Page Front Matter Guide

The following information is applicable to pages created under the `pages` collection (i.e., in the `_pages` folder).
There is a short section on caveats to this at the end.

### Base

The `base` layout includes the boilerplate for each HTML file.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation bar.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation bar. Required if `nav` is `true`.      | Conditional          | Number |

### Single Column Page

The `single_column_page` layout is a general-purpose layout for a variety of pages that is... a single column.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation bar.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation bar. Required if `nav` is `true`.      | Conditional          | Number |
| `show_title`| Set to `true` if the title should be shown as a header at the top of the page. | No (default: `false`)| Boolean|

### About

The `about` layout is specifically for a double-column page with a description on one side and a picture on the other.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation bar.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation bar. Required if `nav` is `true`.      | Conditional          | Number |
| `headshot` | The URL of an image to be displayed as the picture on the about page.           | Yes                  | String |

### Projects

The `projects` layout is specifically for a project showcase page.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation bar.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation bar. Required if `nav` is `true`.      | Conditional          | Number |

### Resume

The `resume` layout is specifically for a resume/CV page.

| Variable    | Description                                                                    | Required             | Type   |
|-------------|--------------------------------------------------------------------------------|----------------------|--------|
| `title`     | The title of the page.                                                         | Yes                  | String |
| `nav`       | Set to `true` if the page should appear in the navigation bar.                 | No (default: `false`)| Boolean|
| `nav_order` | The order of the page in the navigation bar. Required if `nav` is `true`.      | Conditional          | Number |

### Caveats

- All pages created under the `projects` collection (i.e., in the `_projects` folder) are defaulted to have `layout: single_page_column` and `show_title: true`.

## TODOs
N/A 
