# frozen_string_literal: true

require "rubygems/package"
require "tmpdir"
require "rbconfig"

module PackageSmoke
  ROOT = File.expand_path("../../..", __dir__)

  def self.run(package, native:)
    package = File.expand_path(package)
    archive = Gem::Package.new(package)
    archive.verify
    spec = archive.spec
    raise "Wrong package kind" unless (spec.platform != Gem::Platform::RUBY) == native
    raise "Unexpected package" unless spec.name == "audiowaveform"
    raise "Missing native extension declaration" if !native && spec.extensions.empty?
    raise "Native package requires compilation/dependencies" if native && (!spec.extensions.empty? || !spec.dependencies.empty?)
    forbidden = spec.files.grep(%r{\A(?:crates/|fixtures/|target/|tmp/|\.git/|COPYING\z)})
    raise "Legacy/build files in gem: #{forbidden}" unless forbidden.empty?
    raise "Wrong package licenses" unless spec.licenses.sort == ["Apache-2.0", "MIT"]
    %w[LICENSE.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md third-party/manifest.json sig/audiowaveform.rbs].each do |path|
      raise "Missing #{path}" unless spec.files.include?(path)
    end
    binaries = spec.files.grep(/\.(?:bundle|so|dll|dylib)\z/)
    raise "Source gem contains native binary" if !native && !binaries.empty?
    if native
      raise "Native gem lacks a binary" if binaries.empty?
    else
      %w[Cargo.toml src/lib.rs bindings/ruby/ext/audiowaveform/src/lib.rs].each do |path|
        raise "Source gem is incomplete: #{path}" unless spec.files.include?(path)
      end
    end

    Dir.mktmpdir("waveform-gem-install") do |directory|
      # Source installs use already-installed build tools. Native installs must
      # work with an otherwise empty gem environment and no compilation.
      env = ENV.keys.grep(/\ABUNDLER?_/).to_h { |key| [key, nil] }.merge(
        "GEM_HOME" => directory, "GEM_PATH" => native ? directory : ([directory] + Gem.path).join(File::PATH_SEPARATOR),
        "RUBYOPT" => nil, "RUBYLIB" => nil, "RUBYGEMS_GEMDEPS" => nil
      )
      system(env, RbConfig.ruby, "-S", "gem", "install", "--local", "--ignore-dependencies", "--no-document",
        "--install-dir", directory, package, exception: true)
      system(env, RbConfig.ruby, "-e", <<~'CODE', directory, ROOT, spec.version.to_s, exception: true)
        require "json"
        directory, repository, version = ARGV
        gem "audiowaveform", version
        require "audiowaveform"
        abort "wrong version" unless AudioWaveform::VERSION == version
        loaded = $LOADED_FEATURES.find { |path| path.match?(%r{/audiowaveform_ruby\.(bundle|so|dll)\z}) }
        gem_root = File.realpath(Gem.loaded_specs.fetch("audiowaveform").full_gem_path)
        abort "wrong install root" unless gem_root.start_with?(File.realpath(directory) + "/")
        abort "loaded extension outside installed gem" unless loaded && File.realpath(loaded).start_with?(gem_root + "/")
        vectors = JSON.parse(File.read(File.join(repository, "contract/vectors.json")))
        vectors.fetch("cases").each do |vector|
          options = vector.fetch("options").transform_keys(&:to_sym)
          waveform = AudioWaveform.generate(File.join(repository, "tests/fixtures/generated", vector.fetch("fixture") + ".wav"), **options)
          expected = vector.fetch("expected")
          abort "wrong peaks: #{vector.fetch('id')}" unless waveform.data == expected.fetch("data16") && waveform.data(bits: 8) == expected.fetch("data8")
          abort "wrong length" unless waveform.length == expected.fetch("length")
          abort "exports leaked into API" if %i[save to_dat to_txt to_json].any? { |name| AudioWaveform::Waveform.instance_methods(false).include?(name) }
        end
        codec_root = File.join(repository, "tests/fixtures/codecs")
        manifest = JSON.parse(File.read(File.join(codec_root, "manifest.json")))
        manifest.fetch("cases").each do |entry|
          waveform = AudioWaveform.generate(File.join(codec_root, entry.fetch("file")), points: 110)
          abort "codec failed: #{entry.fetch('file')}" unless waveform.length == 110
        end
        puts "Installed #{Gem.loaded_specs.fetch('audiowaveform').full_name}: 36 vectors and codec matrix passed on Ruby #{RUBY_VERSION}"
      CODE
    end
  end
end
