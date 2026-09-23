source "https://rubygems.org"

# Version variables 
jekyll_version = "~> 4.4.1"
jekyll_scholar_version = "~> 7.2.0"
jekyll_feed_version = "~> 0.17"
jekyll_redirect_from_version = "~> 0.16"

tzinfo_version = ">= 1", "< 3"
wdm_version = "~> 0.1.1"
http_parser_version = "~> 0.6.0"

gem "jekyll", jekyll_version

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

group :jekyll_plugins do
  gem "jekyll-scholar", jekyll_scholar_version
  gem "jekyll-feed", jekyll_feed_version
  gem "jekyll-redirect-from", jekyll_redirect_from_version
end
