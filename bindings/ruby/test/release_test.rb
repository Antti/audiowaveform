# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "open3"
require_relative "../script/publish_gems"
require_relative "../lib/audiowaveform/version"

class ReleaseTest < Minitest::Test
  def test_retry_skips_only_the_same_platform_and_checksum
    with_artifact do |spec, path|
      body = {"sha" => Digest::SHA256.file(path).hexdigest, "platform" => "ruby", "yanked" => false}
      assert GemPublishing.already_published?(spec, path, http: response(200, body))
      refute GemPublishing.already_published?(spec, path, http: response(404))

      [{"sha" => "different"}, {"platform" => "x86_64-linux-gnu"}, {"yanked" => true}].each do |change|
        assert_raises(RuntimeError) do
          GemPublishing.already_published?(spec, path, http: response(200, body.merge(change)))
        end
      end
    end
  end

  def test_lookup_failures_do_not_trigger_a_new_upload
    with_artifact do |spec, path|
      [401, 429, 500].each do |code|
        assert_raises(RuntimeError) do
          GemPublishing.already_published?(spec, path, http: response(code))
        end
      end
    end
  end

  def test_native_release_requires_every_supported_ruby_abi
    Dir.mktmpdir do |directory|
      build_package(directory, rubies: %w[3.2 3.3 3.4 4.0])
      output, status = check(directory, "x86_64-linux")
      assert status.success?, output
      output, status = check(directory)
      refute status.success?
      assert_includes output, "Expected 8 gems"
    end

    Dir.mktmpdir do |directory|
      build_package(directory, rubies: %w[3.4])
      output, status = check(directory, "x86_64-linux")
      refute status.success?
      assert_includes output, "Incorrect Ruby ABI binaries"
    end
  end

  def test_native_release_rejects_install_time_compilation
    Dir.mktmpdir do |directory|
      build_package(directory, rubies: %w[3.2 3.3 3.4 4.0], extensions: ["extconf.rb"])
      output, status = check(directory, "x86_64-linux")
      refute status.success?
      assert_includes output, "would compile during installation"
    end
  end

  def test_unlicensed_development_packages_cannot_reach_publishing
    Dir.mktmpdir do |directory|
      platforms = %w[ruby x86_64-linux-gnu aarch64-linux-gnu x86_64-linux-musl
        aarch64-linux-musl x86_64-darwin arm64-darwin x64-mingw-ucrt]
      platforms.each do |platform|
        build_package(directory, rubies: %w[3.2 3.3 3.4 4.0], platform: platform,
          extensions: platform == "ruby" ? ["extconf.rb"] : [])
      end
      # A subprocess prevents network stubbing from affecting other test cases.
      code = <<~'CODE'
        require ARGV.fetch(0)
        class << Net::HTTP
          def start(*)
            abort "publishing reached the network"
          end
        end
        begin
          GemPublishing.publish(ARGV.fetch(1))
          abort "publishing was accepted"
        rescue RuntimeError => error
          abort error.message unless error.message.include?("Release license is pending")
        end
      CODE
      output, status = Open3.capture2e(RbConfig.ruby, "-e", code,
        File.expand_path("../script/publish_gems.rb", __dir__), directory)
      assert status.success?, output
    end
  end

  private

  def with_artifact
    Dir.mktmpdir do |directory|
      path = File.join(directory, "artifact.gem")
      File.write(path, "release artifact")
      spec = Gem::Specification.new("audiowaveform", AudioWaveform::VERSION)
      yield spec, path
    end
  end

  def response(code, body = {})
    result = Net::HTTPResponse::CODE_TO_OBJ.fetch(code.to_s).new("1.1", code.to_s, "test")
    result.define_singleton_method(:body) { JSON.generate(body) }
    connection = Object.new
    connection.define_singleton_method(:get) { |_path| result }
    http = Object.new
    http.define_singleton_method(:start) { |*args, **options, &block| block.call(connection) }
    http
  end

  def build_package(directory, rubies:, extensions: [], platform: "x86_64-linux-gnu")
    Dir.chdir(directory) do
      extension = platform.include?("darwin") ? "bundle" : "so"
      binaries = platform == "ruby" ? [] : rubies.map do |version|
        "bindings/ruby/lib/audiowaveform/#{version}/audiowaveform_ruby.#{extension}"
      end
      files = ["LICENSE-STATUS.md", "THIRD-PARTY-NOTICES.md", "sig/audiowaveform.rbs", *extensions] + binaries
      files.each do |path|
        FileUtils.mkdir_p(File.dirname(path))
        File.write(path, "fixture")
      end
      spec = Gem::Specification.new("audiowaveform", AudioWaveform::VERSION) do |gem|
        gem.summary = "Release validation fixture"
        gem.authors = ["Test"]
        gem.license = "Nonstandard"
        gem.homepage = "https://github.com/Antti/audiowaveform"
        gem.platform = platform
        gem.required_ruby_version = [">= 3.2", "< 4.1.dev"]
        gem.files = files
        gem.extensions = extensions
      end
      capture_io { Gem::Package.build(spec) }
    end
  end

  def check(directory, *platform)
    Open3.capture2e(RbConfig.ruby, File.expand_path("../script/check_release.rb", __dir__), directory, *platform)
  end
end
