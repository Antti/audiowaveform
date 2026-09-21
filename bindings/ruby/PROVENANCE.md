# Ruby integration provenance

The integration was reused with adaptations from personally authored Ruby work
at [`6b5e886f38022e8a74649db80608fa8cafef4ab1`](https://github.com/Antti/audiowaveform-legacy/commit/6b5e886f38022e8a74649db80608fa8cafef4ab1)
in the archived Antti/audiowaveform-legacy repository.
It was introduced by `a606414`, with subsequent Ruby integration/safety/release
changes. The source review found no BBC implementation in this glue. The user
confirmed personal authorship; Git attribution alone is not ownership proof.

The old core, binaries, legacy recordings, root documentation, and serialization
implementations were not copied. The wrapper now calls this replacement core.
Exports were removed; validation, cancellation, peak arrays, package contents,
and tests were adapted. Ruby safety tests also retain the reviewed personal
GC/GVL/subprocess-testing approach, using newly generated or sparse audio. The new contract test was taken from the separately
prepared replacement contract, not from the old core's output. Test media comes
from this project's generated corpus. Release licensing remains pending.

The following SHA-256 hashes record the *input* files before adaptation, not the
resulting files. Build/release logic is retained where applicable; this is not a
claim that the whole integration was rewritten without prior source exposure.

| Input path | Input SHA-256 |
| --- | --- |
| `Gemfile` | `0694f28993c1ef66cf94db5ceee67f8a8227828f7a4cbda7f56729f92eaa5c8f` |
| `Rakefile` | `f2d6a4562963fd2d852c17f3d4b6de1f9c48153bc1552e1a4b92fcca0f275ed0` |
| `audiowaveform.gemspec` | `2a7ce19c2ac5470ba7f54db73a09384f1f4f20a945fba06d2f951a3ac2748cc8` |
| `sig/audiowaveform.rbs` | `ba8b36ac618a2de591938dc2a692278a6aee551cd9855871eee34aba8ddce6a4` |
| `bindings/ruby/Cargo.toml` | `46824b8645f7b480b7229f3bbae64783bfabd103807951a6d4a3bdb9145f6b23` |
| `bindings/ruby/Rakefile` | `aa8359a09efbbeb11e71a9781466a30540b0e2f34088fec5be9db69ed509ab5f` |
| `bindings/ruby/ext/audiowaveform/Cargo.toml` | `35ab7ae09b56184c27470bf0205544ce02dc3f9506e274dde352b670d584d0e1` |
| `bindings/ruby/ext/audiowaveform/build.rs` | `8da395a6878755d1c0627dac06e81b5979641a55b26fe347857e5de22c0c6e26` |
| `bindings/ruby/ext/audiowaveform/extconf.rb` | `c24f1b0e15896ebe53a02fbe7e9ee19fc4457bc3116f2581d765e7afeaea4314` |
| `bindings/ruby/ext/audiowaveform/src/lib.rs` | `229dc4e20b93d68b637b594d30c7efb0e39322062e17e4041d6a610a178eec5b` |
| `bindings/ruby/lib/audiowaveform.rb` | `88b493c9a4529a54c757fd2b91b9046a8444ba48eec10eb6e82adef1bf433d35` |
| `bindings/ruby/lib/audiowaveform/version.rb` | `436d16de34af7ff99969d8863b1c50f8c46ca52f167a7f20522aa8fc710caaf3` |
| `bindings/ruby/script/check_release.rb` | `59f64dbea74a921a7cd7b96c3d160e3871c9d601a1a48b0e871d1410cc2841de` |
| `bindings/ruby/script/publish_gems.rb` | `da4fc7e8235d1cb9c3b561c85157a352f4d5a7f5f13b5de5c0896dcdf8ace425` |
| `bindings/ruby/test/release_test.rb` | `b6fede6b9182f416f83463958239ddc08a9bd3efcf139c29fbb93c65f85ef6bb` |
| `.github/workflows/ruby.yml` | `c9d9589e9869386cc2fefbe16b4a50792e8df11fed4ab6119b047799a50f9760` |
| `.github/workflows/ruby-native.yml` | `7d720e0646e61da23976ed8ff1f151339a5bdbb0abbd822e22a74408f634e9f8` |
