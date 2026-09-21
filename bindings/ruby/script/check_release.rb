# frozen_string_literal: true

require "rubygems/package"
require_relative "../lib/audiowaveform/version"

# Check the entire release before any upload, including the exact Ruby ABI files.
platforms = %w[
  ruby x86_64-linux-gnu aarch64-linux-gnu x86_64-linux-musl
  aarch64-linux-musl x86_64-darwin arm64-darwin x64-mingw-ucrt
]
directory = ARGV.fetch(0)
if (platform = ARGV[1])
  platform += "-gnu" if %w[x86_64-linux aarch64-linux].include?(platform)
  abort "Unknown release platform: #{platform}" unless platforms.include?(platform)
  platforms = [platform]
end

files = Dir[File.join(directory, "*.gem")].sort
abort "Expected #{platforms.length} gems, found #{files.length}" unless files.length == platforms.length

files.each do |path|
  package = Gem::Package.new(path)
  package.verify
  spec = package.spec
  abort "Unexpected gem: #{spec.full_name}" unless spec.name == "audiowaveform" &&
    spec.version.to_s == AudioWaveform::VERSION
  abort "Unexpected or duplicate platform: #{spec.platform}" unless platforms.delete(spec.platform.to_s)
  abort "Missing license or signatures: #{path}" unless %w[LICENSE.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md sig/audiowaveform.rbs].all? { |file| spec.files.include?(file) }

  binaries = spec.files.grep(/\.(?:bundle|so|dll)\z/)
  if spec.platform == Gem::Platform::RUBY
    abort "Source gem includes native binaries" unless binaries.empty?
    abort "Source gem is missing its build extension" if spec.extensions.empty?
  else
    abort "Native gem would compile during installation" unless spec.extensions.empty?
    abort "Native gem depends on rb_sys" if spec.dependencies.any? { |dependency| dependency.name == "rb_sys" }
    extension = spec.platform.os == "darwin" ? "bundle" : "so"
    expected = %w[3.2 3.3 3.4 4.0].map do |version|
      "bindings/ruby/lib/audiowaveform/#{version}/audiowaveform_ruby.#{extension}"
    end
    abort "Incorrect Ruby ABI binaries in #{path}: #{binaries.inspect}" unless binaries.sort == expected.sort
    unless spec.required_ruby_version.satisfied_by?(Gem::Version.new("3.2.0")) &&
        spec.required_ruby_version.satisfied_by?(Gem::Version.new("4.0.0")) &&
        !spec.required_ruby_version.satisfied_by?(Gem::Version.new("4.1.0"))
      abort "Incorrect Ruby version requirements in #{path}"
    end
  end
  puts "Verified #{spec.full_name}"
end
