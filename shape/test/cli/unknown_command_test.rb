class UnknownCommandTest < AcceptanceTest
  def setup = workspace("atelier")

  test "an unknown command is refused as a mistake in the command" do
    cli "frobnicate"

    assert_exit 2
    assert_nothing_written
  end
end
