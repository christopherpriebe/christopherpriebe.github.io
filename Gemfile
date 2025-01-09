source "https://rubygems.org"

# Hello! This is where you manage which Jekyll version is used to run.
# When you want to use a different version, change it below, save the
# file and run `bundle install`. Run Jekyll with `bundle exec`, like so:
#
#     bundle exec jekyll serve
#

# Version variables 
jekyll_version = "~> 3.9.5"
minima_version = "~> 2.5"
github_pages_version = "~> 231"
jekyll_feed_version = "~> 0.17"
tzinfo_version = ">= 1", "< 3"
wdm_version = "~> 0.1.1"
http_parser_version = "~> 0.6.0"

# This will help ensure the proper Jekyll version is running.
# Happy Jekylling!
# gem "jekyll", jekyll_version

# If you want to use GitHub Pages, remove the "gem "jekyll"" above and
# uncomment the line below. To upgrade, run `bundle update github-pages`.
gem "github-pages", github_pages_version, group: :jekyll_plugins

# If you have any plugins, put them here!
# group :jekyll_plugins do
#   gem "jekyll-feed", jekyll_feed_version
# end

# Windows and JRuby does not include zoneinfo files, so bundle the tzinfo-data gem
# and associated library.
platforms :mingw, :x64_mingw, :mswin, :jruby do
  gem "tzinfo", tzinfo_version
  gem "tzinfo-data"
end

# Performance-booster for watching directories on Windows
gem "wdm", wdm_version, :platforms => [:mingw, :x64_mingw, :mswin]

# Lock `http_parser.rb` gem to `v0.6.x` on JRuby builds since newer versions of the gem
# do not have a Java counterpart.
gem "http_parser.rb", http_parser_version, :platforms => [:jruby]

gem "jekyll-scholar", group: :jekyll_plugins
