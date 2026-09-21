# frozen_string_literal: true
require_relative "package_smoke"
packages = ARGV.empty? ? Dir[File.join(PackageSmoke::ROOT, "pkg/*.gem")] : ARGV
abort "Expected one native gem, found #{packages.length}" unless packages.length == 1
PackageSmoke.run(packages.fetch(0), native: true)
