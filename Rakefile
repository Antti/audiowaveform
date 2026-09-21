# frozen_string_literal: true

require "fileutils"
require "rubygems/package"
require "rake/testtask"
require "rb_sys/extensiontask"

GEMSPEC = Gem::Specification.load(File.join(__dir__, "audiowaveform.gemspec"))

# The extension belongs to a separate Cargo workspace, but gems are packaged
# from the repository root so their sources, license, and signatures stay together.
class AudioWaveformExtensionTask < RbSys::ExtensionTask
  def cargo_metadata
    @cargo_metadata ||= Dir.chdir(File.join(__dir__, "bindings/ruby")) do
      RbSys::Cargo::Metadata.new("audiowaveform-ruby").load!
    end
  end
end

AudioWaveformExtensionTask.new("audiowaveform-ruby", GEMSPEC) do |extension|
  extension.lib_dir = "bindings/ruby/lib/audiowaveform"
  # When host and target match, omit the host-only binary from a cross gem.
  extension.no_native = true if ENV.key?("RUBY_TARGET")
  extension.cross_compiling do |spec|
    # A generic linux gem also matches musl in RubyGems; label glibc explicitly.
    if spec.platform.os == "linux" && spec.platform.version.nil?
      spec.platform = Gem::Platform.new("#{spec.platform.cpu}-linux-gnu")
      # RubyGems 3.x serializes the first assigned platform unless reset explicitly.
      spec.original_platform = spec.platform.to_s
      spec.required_rubygems_version = ">= 3.3.22"
    end
    spec.files.select! do |path|
      path.start_with?("bindings/ruby/lib/", "sig/", "third-party/") ||
        %w[LICENSE.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md PROVENANCE.md VALIDATION.md contract/SPEC.md README.md bindings/ruby/README.md bindings/ruby/CHANGELOG.md bindings/ruby/PROVENANCE.md].include?(path)
    end
  end
end

Rake::TestTask.new(:test) do |test|
  test.libs << File.expand_path("bindings/ruby/lib", __dir__)
  test.pattern = File.expand_path("bindings/ruby/test/**/*_test.rb", __dir__)
end

task test: :compile

desc "Build the Ruby source gem"
task :build do
  FileUtils.mkdir_p("pkg")
  gem_file = Gem::Package.build(GEMSPEC)
  FileUtils.mv(gem_file, "pkg")
end

desc "Validate the bundled RBS signature"
task :rbs do
  sh "bundle", "exec", "rbs", "-I", "sig", "validate"
end

task default: %i[compile test rbs]
