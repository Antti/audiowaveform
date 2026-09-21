# frozen_string_literal: true

require_relative "bindings/ruby/lib/audiowaveform/version"

Gem::Specification.new do |spec|
  spec.name = "audiowaveform"
  spec.version = AudioWaveform::VERSION
  spec.authors = ["Andrii Dmytrenko"]
  spec.summary = "Streaming audio waveform peaks for Ruby, powered by Rust."
  spec.description = "Generate mono or multichannel min/max peaks with exact point counts and direct 8- or 16-bit arrays."
  spec.homepage = "https://github.com/Antti/audiowaveform"
  # Either license may be chosen; see LICENSE.md.
  spec.licenses = ["MIT", "Apache-2.0"]
  spec.required_ruby_version = ">= 3.2"
  spec.metadata["allowed_push_host"] = "https://rubygems.org"
  spec.metadata["cargo_crate_name"] = "audiowaveform-ruby"
  spec.metadata["source_code_uri"] = spec.homepage
  spec.metadata["rubygems_mfa_required"] = "true"

  # Explicit source patterns exclude local binaries, build output, tests, and
  # the former repository even when building from an uncommitted checkout.
  spec.files = Dir.chdir(__dir__) do
    Dir[
      "Cargo.toml", "Cargo.lock", "src/**/*.rs", "README.md", "PROVENANCE.md",
      "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md", "third-party/**/*",
      "VALIDATION.md", "contract/SPEC.md",
      "bindings/ruby/Cargo.toml", "bindings/ruby/Cargo.lock",
      "bindings/ruby/README.md", "bindings/ruby/CHANGELOG.md", "bindings/ruby/PROVENANCE.md",
      "bindings/ruby/ext/audiowaveform/Cargo.toml",
      "bindings/ruby/ext/audiowaveform/build.rs",
      "bindings/ruby/ext/audiowaveform/extconf.rb",
      "bindings/ruby/ext/audiowaveform/src/**/*.rs",
      "bindings/ruby/lib/**/*.rb", "sig/**/*.rbs"
    ].select { |path| File.file?(path) }.sort
  end
  spec.require_paths = ["bindings/ruby/lib"]
  spec.extensions = ["bindings/ruby/ext/audiowaveform/extconf.rb"]
  spec.add_dependency "rb_sys", "~> 0.9"
end
