# frozen_string_literal: true
require_relative "package_smoke"
abort "Usage: ruby test_source_gem.rb SOURCE_GEM" unless ARGV.length == 1
PackageSmoke.run(ARGV.fetch(0), native: false)
