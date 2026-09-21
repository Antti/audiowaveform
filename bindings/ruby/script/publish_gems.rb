# frozen_string_literal: true

require "digest"
require "json"
require "net/http"
require "rbconfig"
require "rubygems/package"

# Re-running a failed publishing job must not try to overwrite an uploaded gem.
# Only skip an existing version/platform when its SHA-256 matches this artifact.
module GemPublishing
  def self.already_published?(spec, path, http: Net::HTTP)
    uri = URI("https://rubygems.org/api/v2/rubygems/#{spec.name}/versions/#{spec.version}.json")
    uri.query = URI.encode_www_form(platform: spec.platform.to_s)
    response = http.start(uri.host, uri.port, use_ssl: true, open_timeout: 15, read_timeout: 30) do |connection|
      connection.get(uri.request_uri)
    end
    return false if response.is_a?(Net::HTTPNotFound)
    raise "RubyGems lookup failed: HTTP #{response.code}" unless response.is_a?(Net::HTTPSuccess)

    remote = JSON.parse(response.body)
    unless remote.fetch("sha") == Digest::SHA256.file(path).hexdigest &&
        remote.fetch("platform") == spec.platform.to_s && !remote.fetch("yanked", false)
      raise "RubyGems already has a different or yanked #{spec.full_name}; use a new version"
    end
    true
  end

  def self.publish(directory)
    # This check runs before the first network request or mutation.
    system(RbConfig.ruby, File.join(__dir__, "check_release.rb"), directory, exception: true)
    paths = Dir[File.join(directory, "*.gem")].sort
    # Development packages must never be uploaded before licensing is settled.
    paths.each do |path|
      spec = Gem::Package.new(path).spec
      if spec.metadata["release_status"] == "license-review-pending" || spec.licenses.include?("Nonstandard")
        raise "Release license is pending for #{spec.full_name}"
      end
    end
    # Check every artifact for conflicts before publishing any of them.
    pending = paths.reject do |path|
      spec = Gem::Package.new(path).spec
      already_published?(spec, path).tap do |published|
        puts "Already published #{spec.full_name} (matching SHA-256)" if published
      end
    end
    pending.each do |path|
      system(RbConfig.ruby, "-S", "gem", "push", path, "--host", "https://rubygems.org", exception: true)
    end
  end
end

GemPublishing.publish(ARGV.fetch(0)) if $PROGRAM_NAME == __FILE__
